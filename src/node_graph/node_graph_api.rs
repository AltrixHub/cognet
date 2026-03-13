#[cfg(not(target_arch = "wasm32"))]
use tokio::{runtime::Handle, task};

#[cfg(target_arch = "wasm32")]
use futures::future::join_all;

use async_trait::async_trait;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use crate::{
    node_graph_system::NodeGraphSystem, ColorValue, Data, DataType, DataValue, Edge, EdgeId,
    ErrorTarget, ExecutionContext, GraphError, NodeEntity, NodeGraph, NodeId, NodeImpl, NodeMeta,
    OutputWriter, SharedExecutionCache, SharedNodeStates, SubGraphNode, Vector3,
};

/// Execution event for progress tracking during graph execution.
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Node execution started.
    Started(NodeId),
    /// Node execution completed successfully.
    Completed(NodeId),
    /// Node execution failed with an error.
    Failed(NodeId, String),
}

/// Output from a single node execution.
#[derive(Debug, Clone)]
pub struct NodeOutput {
    /// The node that produced this output.
    pub node_id: NodeId,
    /// Output slot index.
    pub slot_index: usize,
    /// The output data (Arc-shared with ExecutionCache for zero-copy).
    pub data: Data,
}

/// Value flowing through an edge after execution.
#[derive(Debug, Clone)]
pub struct EdgeValue {
    pub from_node: NodeId,
    pub from_slot: usize,
    pub to_node: NodeId,
    pub to_slot: usize,
    pub data: Option<Data>,
}

/// Error from a single node execution.
#[derive(Debug, Clone)]
pub struct NodeError {
    pub message: String,
}

/// Result of a graph execution.
///
/// Contains all outputs from nodes that were (re)computed, edge values,
/// execution errors, and removed nodes.
#[derive(Debug, Clone, Default)]
pub struct ExecutionResult {
    /// Outputs from nodes computed in this execution (new + updated).
    pub node_outputs: HashMap<NodeId, Vec<Option<Data>>>,
    /// Values flowing through edges after execution.
    pub edge_values: HashMap<EdgeId, EdgeValue>,
    /// Execution errors per node.
    pub errors: HashMap<NodeId, NodeError>,
    /// Nodes removed since the last execute() call.
    pub removed_nodes: Vec<NodeId>,
    /// Raw outputs for backward compatibility during migration.
    pub outputs: Vec<NodeOutput>,
}

impl ExecutionResult {
    /// Create empty result.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty() && self.removed_nodes.is_empty()
    }
}

/// Backward-compatible type alias during migration.
pub type GraphChanges = ExecutionResult;

/// Build an `ExecutionContext` for a node.
///
/// Resolves all input values from upstream outputs (with default value fallback)
/// using NodeStates for edge topology and ExecutionCache for output values.
/// Captures node_data and creates an OutputWriter for the node's output slots.
fn build_execution_context(
    node_id: &NodeId,
    node_states: &SharedNodeStates,
    cache: &SharedExecutionCache,
) -> Result<ExecutionContext, String> {
    let ns = node_states.read().map_err(|e| e.to_string())?;
    let cache_read = cache.read()?;

    // Resolve input values from NodeStates slot metadata
    let input_count = ns.input_slot_count(node_id);
    let input_values = (0..input_count)
        .map(|idx| {
            let mut values = Vec::new();
            if let Some(slot_state) = ns.input_slot(node_id, idx) {
                for edge_id in ns.edges_for_input(&slot_state.id) {
                    if let Some(edge) = ns.get_edge(edge_id) {
                        if let Some(data) = cache_read.outputs.get(&edge.from_output_slot_id) {
                            values.push(data.share());
                        }
                    }
                }
                // Default value fallback when no edges are connected
                if values.is_empty() {
                    if let Some(default_ref) = slot_state.default_value.as_ref() {
                        if let Ok(data) = Data::from_any(Arc::clone(default_ref)) {
                            values.push(data);
                        }
                    }
                }
            }
            values
        })
        .collect();

    // Capture node data from NodeStates
    let node_data = ns
        .get(node_id)
        .and_then(|state| state.data.as_ref().map(|d| d.share()));

    // Build output writer from NodeStates output slot metadata
    let output_count = ns.output_slot_count(node_id);
    let slots = (0..output_count)
        .filter_map(|idx| ns.output_slot(node_id, idx).map(|s| (s.id, s.data_type)))
        .collect();
    let output_writer = OutputWriter::new(cache.share(), slots);

    Ok(ExecutionContext {
        node_data,
        input_values,
        output_writer,
    })
}

/// Extract field name/value pairs from a `Data` value for composite types.
fn extract_fields_from_data(data_type: DataType, data: Option<&Data>) -> Vec<(&'static str, f64)> {
    let fields = data_type.field_names();
    if fields.is_empty() {
        return Vec::new();
    }
    let Some(data) = data else {
        return fields.iter().map(|&f| (f, 0.0)).collect();
    };
    match data_type {
        DataType::Vector3 => {
            if let Ok(v) = data.value::<Vector3>() {
                vec![("x", v.x), ("y", v.y), ("z", v.z)]
            } else {
                fields.iter().map(|&f| (f, 0.0)).collect()
            }
        }
        DataType::Color => {
            if let Ok(c) = data.value::<ColorValue>() {
                vec![("r", c.r), ("g", c.g), ("b", c.b), ("a", c.a)]
            } else {
                fields.iter().map(|&f| (f, 0.0)).collect()
            }
        }
        _ => fields.iter().map(|&f| (f, 0.0)).collect(),
    }
}

/// Extract field name/value pairs from a `DataValue` (slot default) for composite types.
fn extract_fields_from_default_value(
    data_type: DataType,
    default_value: Option<&DataValue>,
) -> Vec<(&'static str, f64)> {
    let fields = data_type.field_names();
    if fields.is_empty() {
        return Vec::new();
    }
    let Some(dv) = default_value else {
        return fields.iter().map(|&f| (f, 0.0)).collect();
    };
    match data_type {
        DataType::Vector3 => {
            if let Some(v) = dv.downcast_ref::<Vector3>() {
                vec![("x", v.x), ("y", v.y), ("z", v.z)]
            } else {
                fields.iter().map(|&f| (f, 0.0)).collect()
            }
        }
        DataType::Color => {
            if let Some(c) = dv.downcast_ref::<ColorValue>() {
                vec![("r", c.r), ("g", c.g), ("b", c.b), ("a", c.a)]
            } else {
                fields.iter().map(|&f| (f, 0.0)).collect()
            }
        }
        _ => fields.iter().map(|&f| (f, 0.0)).collect(),
    }
}

#[async_trait]
pub trait NodeGraphAPI {
    fn new() -> Result<Self, String>
    where
        Self: Sized;

    /// Execute the graph asynchronously.
    ///
    /// Returns `ExecutionResult` containing outputs from computed nodes,
    /// edge values, execution errors, and IDs of removed nodes.
    async fn execute(&self) -> Result<ExecutionResult, String>;

    /// Execute the graph with progress callback.
    ///
    /// The callback is called for each node as it starts executing,
    /// completes, or fails. This enables real-time UI updates.
    ///
    /// Returns `ExecutionResult` containing outputs from computed nodes,
    /// edge values, execution errors, and IDs of removed nodes.
    async fn execute_with_progress<F>(&self, on_progress: F) -> Result<ExecutionResult, String>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static;

    // Sync methods for graph structure operations
    fn create_node<T: NodeImpl + NodeMeta + 'static>(&mut self) -> Result<NodeId, String>;
    fn create_node_by_name(&mut self, name: &str) -> Result<NodeId, String>;
    fn create_node_by_name_with_id(&mut self, id: NodeId, name: &str) -> Result<NodeId, String>;
    fn remove_node(&mut self, node_id: NodeId) -> Result<(), String>;
    fn update_node_data(&mut self, node_id: &NodeId, data: Data) -> Result<(), String>;
    fn update_input_slot_default_data(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String>;
    fn update_node_data_field(
        &mut self,
        node_id: &NodeId,
        data_type: DataType,
        field_name: &str,
        value: f64,
    ) -> Result<(), String>;
    fn update_input_slot_default_field(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        field_name: &str,
        value: f64,
    ) -> Result<(), String>;
    fn get_node_by_id(&self, node_id: &NodeId) -> Option<NodeEntity>;
    fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId>;
    fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)>;
    fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String>;
    fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), String>;
    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, String>;
    fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<Data>;
    fn node_variants(&self) -> &HashSet<String>;
}

#[async_trait]
impl NodeGraphAPI for NodeGraph {
    fn new() -> Result<Self, String> {
        NodeGraph::new()
    }

    async fn execute(&self) -> Result<ExecutionResult, String> {
        self.execute_with_progress(|_| {}).await
    }

    async fn execute_with_progress<F>(&self, on_progress: F) -> Result<ExecutionResult, String>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static,
    {
        // Drain removed list from bookkeeping
        let removed = {
            let mut b = self.bookkeeping.lock().map_err(|e| e.to_string())?;
            std::mem::take(&mut b.removed_since_last_execute)
        };

        // Drain changed nodes from NodeStates
        let changed = {
            let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
            ns.drain_changed_nodes()
        }; // write lock dropped

        tracing::debug!(
            target: "graph",
            "[cognet] execute: changed={} removed={}",
            changed.len(),
            removed.len(),
        );

        if changed.is_empty() && removed.is_empty() {
            return Ok(ExecutionResult::empty());
        }

        if changed.is_empty() {
            return Ok(ExecutionResult {
                removed_nodes: removed,
                ..Default::default()
            });
        }

        // Clear previous execution errors before running
        self.clear_execution_errors();

        // BFS: changed nodes → all downstream nodes
        let dirty_nodes: HashSet<NodeId> = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let mut affected = HashSet::new();
            let mut queue: VecDeque<NodeId> = changed.into_iter().collect();
            while let Some(node_id) = queue.pop_front() {
                if affected.insert(node_id) {
                    for edge_id in ns.outgoing_edges_for_node(&node_id) {
                        if let Some(edge) = ns.get_edge(edge_id) {
                            if !affected.contains(&edge.to_node_id) {
                                queue.push_back(edge.to_node_id);
                            }
                        }
                    }
                }
            }
            affected
        };

        let sorted_node_levels = self.topological_sort(&dirty_nodes)?;

        // Collect all node IDs that will be executed (for output collection)
        let executed_node_ids: Vec<NodeId> = sorted_node_levels
            .iter()
            .flat_map(|level| level.iter().copied())
            .collect();

        let shared_nodes = self.node_manager.nodes();
        let shared_cache = self.cache.share();
        let shared_node_states = self.node_states.clone();
        let on_progress = Arc::new(on_progress);

        #[cfg(target_arch = "wasm32")]
        {
            for level_nodes in sorted_node_levels {
                let futures: Vec<_> = level_nodes
                    .into_iter()
                    .map(|node_id| {
                        let shared_nodes = shared_nodes.share();
                        let shared_cache = shared_cache.share();
                        let shared_node_states = shared_node_states.clone();
                        let on_progress = Arc::clone(&on_progress);
                        async move {
                            on_progress(ExecutionEvent::Started(node_id));

                            let result = {
                                let nodes_guard = match shared_nodes.lock() {
                                    Ok(g) => g,
                                    Err(_) => return (node_id, Err("Lock poisoned".to_string())),
                                };
                                if let Some(node) = nodes_guard.get(&node_id) {
                                    let node_clone = Arc::clone(node);
                                    drop(nodes_guard);
                                    let node_read = match node_clone.read() {
                                        Ok(r) => r,
                                        Err(_) => {
                                            return (node_id, Err("Node lock poisoned".to_string()))
                                        }
                                    };
                                    if let Some(sg) =
                                        node_read.as_any().downcast_ref::<SubGraphNode>()
                                    {
                                        sg.execute_internal(
                                            &node_id,
                                            shared_cache.share(),
                                            shared_node_states.clone(),
                                        )
                                        .await
                                    } else {
                                        match build_execution_context(
                                            &node_id,
                                            &shared_node_states,
                                            &shared_cache,
                                        ) {
                                            Ok(ctx) => node_read.execute(ctx).await,
                                            Err(e) => Err(e),
                                        }
                                    }
                                } else {
                                    Ok(())
                                }
                            };

                            match &result {
                                Ok(()) => on_progress(ExecutionEvent::Completed(node_id)),
                                Err(msg) => {
                                    on_progress(ExecutionEvent::Failed(node_id, msg.clone()))
                                }
                            }

                            (node_id, result)
                        }
                    })
                    .collect();

                let results = join_all(futures).await;
                for (node_id, res) in results {
                    if let Err(msg) = res {
                        self.add_error(GraphError::execution(node_id, msg));
                    }
                }
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let rt_handle = Arc::new(Handle::current());
            for level_nodes in sorted_node_levels {
                let results: Vec<(NodeId, Result<(), String>)> = task::spawn_blocking({
                    let shared_nodes = shared_nodes.share();
                    let shared_cache = shared_cache.share();
                    let shared_node_states = shared_node_states.clone();
                    let rt_handle = Arc::clone(&rt_handle);
                    let on_progress = Arc::clone(&on_progress);
                    move || {
                        level_nodes
                            .into_par_iter()
                            .map(|node_id| {
                                on_progress(ExecutionEvent::Started(node_id));

                                let result = {
                                    let nodes_guard = match shared_nodes.lock() {
                                        Ok(g) => g,
                                        Err(_) => {
                                            return (node_id, Err("Lock poisoned".to_string()))
                                        }
                                    };
                                    if let Some(node) = nodes_guard.get(&node_id) {
                                        let node_clone = Arc::clone(node);
                                        let cache_clone = shared_cache.share();
                                        let ns_clone = shared_node_states.clone();
                                        let rt_clone = Arc::clone(&rt_handle);
                                        drop(nodes_guard);
                                        // Catch panics so a single node failure
                                        // doesn't abort the entire execution level.
                                        match std::panic::catch_unwind(
                                            std::panic::AssertUnwindSafe(|| {
                                                #[allow(clippy::await_holding_lock)]
                                                rt_clone.block_on(async {
                                                    // Read lock held across await — intentional.
                                                    // execute() takes &self, so the read guard
                                                    // must live for the duration of the call.
                                                    let node_read = match node_clone.read() {
                                                        Ok(r) => r,
                                                        Err(poisoned) => poisoned.into_inner(),
                                                    };
                                                    if let Some(sg) = node_read
                                                        .as_any()
                                                        .downcast_ref::<SubGraphNode>()
                                                    {
                                                        sg.execute_internal(
                                                            &node_id,
                                                            cache_clone,
                                                            ns_clone,
                                                        )
                                                        .await
                                                    } else {
                                                        match build_execution_context(
                                                            &node_id,
                                                            &ns_clone,
                                                            &cache_clone,
                                                        ) {
                                                            Ok(ctx) => node_read.execute(ctx).await,
                                                            Err(e) => Err(e),
                                                        }
                                                    }
                                                })
                                            }),
                                        ) {
                                            Ok(result) => result,
                                            Err(panic_payload) => {
                                                let msg = if let Some(s) =
                                                    panic_payload.downcast_ref::<&str>()
                                                {
                                                    format!("Node panicked: {}", s)
                                                } else if let Some(s) =
                                                    panic_payload.downcast_ref::<String>()
                                                {
                                                    format!("Node panicked: {}", s)
                                                } else {
                                                    "Node panicked".to_string()
                                                };
                                                Err(msg)
                                            }
                                        }
                                    } else {
                                        Ok(())
                                    }
                                };

                                match &result {
                                    Ok(()) => on_progress(ExecutionEvent::Completed(node_id)),
                                    Err(msg) => {
                                        on_progress(ExecutionEvent::Failed(node_id, msg.clone()))
                                    }
                                }

                                (node_id, result)
                            })
                            .collect()
                    }
                })
                .await
                .map_err(|e| e.to_string())?;

                for (node_id, res) in results {
                    if let Err(msg) = res {
                        self.add_error(GraphError::execution(node_id, msg));
                    }
                }
            }
        }

        // Collect outputs from executed nodes (Arc-shared for zero-copy)
        let mut outputs = Vec::new();
        let mut node_outputs: HashMap<NodeId, Vec<Option<Data>>> = HashMap::new();
        let mut edge_values: HashMap<EdgeId, EdgeValue> = HashMap::new();

        if let Ok(ns) = shared_node_states.read() {
            if let Ok(cache) = shared_cache.read() {
                for node_id in &executed_node_ids {
                    let output_count = ns.output_slot_count(node_id);
                    let mut slot_outputs = vec![None; output_count];
                    for (idx, slot_out) in slot_outputs.iter_mut().enumerate() {
                        if let Some(slot) = ns.output_slot(node_id, idx) {
                            if let Some(data) = cache.outputs.get(&slot.id) {
                                *slot_out = Some(data.share());
                                outputs.push(NodeOutput {
                                    node_id: *node_id,
                                    slot_index: idx,
                                    data: data.share(),
                                });
                            }
                        }
                    }

                    // For sink nodes (0 outputs), resolve input values via edges
                    // so display/output nodes can show their received values.
                    if output_count == 0 {
                        let input_count = ns.input_slot_count(node_id);
                        let mut slot_inputs = vec![None; input_count];
                        for edge in ns.edges().values() {
                            if edge.to_node_id == *node_id
                                && (edge.to_input_slot_index) < slot_inputs.len()
                            {
                                if let Some(data) = cache.outputs.get(&edge.from_output_slot_id) {
                                    slot_inputs[edge.to_input_slot_index] = Some(data.share());
                                }
                            }
                        }
                        node_outputs.insert(*node_id, slot_inputs);
                    } else {
                        node_outputs.insert(*node_id, slot_outputs);
                    }
                }

                // Build edge values from all edges involving executed nodes
                for (edge_id, edge) in ns.edges() {
                    if executed_node_ids.contains(&edge.from_node_id) {
                        let data = cache
                            .outputs
                            .get(&edge.from_output_slot_id)
                            .map(|d| d.share());
                        edge_values.insert(
                            *edge_id,
                            EdgeValue {
                                from_node: edge.from_node_id,
                                from_slot: edge.from_output_slot_index,
                                to_node: edge.to_node_id,
                                to_slot: edge.to_input_slot_index,
                                data,
                            },
                        );
                    }
                }
            }
        }

        // Extract execution errors for nodes
        let mut errors: HashMap<NodeId, NodeError> = HashMap::new();
        let all_errors = self.errors();
        for (target, err) in &all_errors {
            if let ErrorTarget::Node(node_id) = target {
                if err.is_execution_error() {
                    errors.insert(
                        *node_id,
                        NodeError {
                            message: err.message(),
                        },
                    );
                }
            }
        }

        Ok(ExecutionResult {
            node_outputs,
            edge_values,
            errors,
            removed_nodes: removed,
            outputs,
        })
    }

    fn create_node<T: NodeImpl + NodeMeta + 'static>(&mut self) -> Result<NodeId, String> {
        let node_id = self.node_manager.create_node::<T>()?;

        // Register in NodeStates
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_node(
                node_id,
                T::NAME,
                T::DEFAULT_VALUE.to_data(),
                T::INPUTS,
                T::OUTPUTS,
            );
        }

        Ok(node_id)
    }

    fn create_node_by_name(&mut self, name: &str) -> Result<NodeId, String> {
        // Create node and get default data from factory
        let (node_id, default_data) = self.node_manager.create_node_by_name(name)?;

        // Get type info for slot counts
        let type_info = crate::get_node_type_info(name)
            .ok_or_else(|| format!("NodeTypeInfo not found for '{}'", name))?;

        // Register in NodeStates
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_node(
                node_id,
                type_info.name,
                default_data,
                type_info.inputs,
                type_info.outputs,
            );
        }

        Ok(node_id)
    }

    fn create_node_by_name_with_id(&mut self, id: NodeId, name: &str) -> Result<NodeId, String> {
        let (_node_id, default_data) = self.node_manager.create_node_by_name_with_id(id, name)?;

        let type_info = crate::get_node_type_info(name)
            .ok_or_else(|| format!("NodeTypeInfo not found for '{}'", name))?;

        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_node(
                id,
                type_info.name,
                default_data,
                type_info.inputs,
                type_info.outputs,
            );
        }

        Ok(id)
    }

    fn remove_node(&mut self, node_id: NodeId) -> Result<(), String> {
        if self.node_manager.node_remove(&node_id).is_some() {
            {
                let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
                guard.remove_node(&node_id); // remove_edge auto-tracks downstream
            }
            self.record_removal(node_id);
            Ok(())
        } else {
            Err("Node not found.".to_string())
        }
    }

    fn update_node_data(&mut self, node_id: &NodeId, data: Data) -> Result<(), String> {
        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
        let node_state = guard
            .get_mut(node_id)
            .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;
        node_state.data = Some(data);
        guard.mark_changed(*node_id);
        Ok(())
    }

    fn update_input_slot_default_data(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String> {
        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
        let slot = guard
            .input_slot_mut(node_id, slot_index)
            .ok_or_else(|| format!("Input slot {} not found for node {:?}", slot_index, node_id))?;
        slot.default_value = Some(data.into_value());
        guard.mark_changed(*node_id);
        Ok(())
    }

    fn update_node_data_field(
        &mut self,
        node_id: &NodeId,
        data_type: DataType,
        field_name: &str,
        value: f64,
    ) -> Result<(), String> {
        // Read current field values from existing node data
        let current_fields: Vec<(&str, f64)> = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let state = ns
                .get(node_id)
                .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;
            extract_fields_from_data(data_type, state.data.as_ref())
        };

        // Assemble new Data by replacing the target field
        let data = data_type
            .assemble(|f| {
                if f == field_name {
                    value
                } else {
                    current_fields
                        .iter()
                        .find(|(name, _)| *name == f)
                        .map(|(_, v)| *v)
                        .unwrap_or(0.0)
                }
            })
            .ok_or_else(|| {
                format!(
                    "Cannot assemble data for type {:?} (non-composite type)",
                    data_type
                )
            })?;

        self.update_node_data(node_id, data)
    }

    fn update_input_slot_default_field(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        field_name: &str,
        value: f64,
    ) -> Result<(), String> {
        // Read current field values and data_type from existing slot default
        let (data_type, current_fields) = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let slot = ns.input_slot(node_id, slot_index).ok_or_else(|| {
                format!("Input slot {} not found for node {:?}", slot_index, node_id)
            })?;
            let dt = slot.data_type;
            let fields = extract_fields_from_default_value(dt, slot.default_value.as_ref());
            (dt, fields)
        };

        // Assemble new Data by replacing the target field
        let data = data_type
            .assemble(|f| {
                if f == field_name {
                    value
                } else {
                    current_fields
                        .iter()
                        .find(|(name, _)| *name == f)
                        .map(|(_, v)| *v)
                        .unwrap_or(0.0)
                }
            })
            .ok_or_else(|| {
                format!(
                    "Cannot assemble data for type {:?} (non-composite type)",
                    data_type
                )
            })?;

        self.update_input_slot_default_data(node_id, slot_index, data)
    }

    fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String> {
        let edge = self
            .create_edge(
                from_node_id,
                from_output_slot_index,
                to_node_id,
                to_input_slot_index,
            )
            .map_err(|e| e.to_string())?;

        self.add_edge(edge)
    }

    fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), String> {
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.remove_edge(&edge_id)
            .ok_or(format!("Edge does not exist: id: {:?}", edge_id))?;
        Ok(())
    }

    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, String> {
        let ns = self.node_states.read().map_err(|e| e.to_string())?;
        ns.get_edge(&edge_id)
            .cloned()
            .ok_or(format!("Edge not found: id {:?}", edge_id))
    }

    fn get_node_by_id(&self, node_id: &NodeId) -> Option<NodeEntity> {
        self.node_manager.get_node_by_id(node_id)
    }

    fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId> {
        self.node_manager.get_node_ids_by_type::<T>()
    }

    fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)> {
        self.node_manager.get_nodes_by_ids(ids)
    }

    fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<Data> {
        let ns = self.node_states.read().ok()?;
        let slot = ns.output_slot(node_id, output_slot_index)?;
        let slot_id = slot.id;
        drop(ns);

        let cache = self.cache.read().ok()?;
        cache.outputs.get(&slot_id).map(|data| data.share())
    }

    fn node_variants(&self) -> &HashSet<String> {
        self.node_manager.variants()
    }
}
