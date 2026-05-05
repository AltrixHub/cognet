use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use std::any::TypeId;

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

/// Execution scheduling mode for `NodeGraph::execute_sync_with_mode`.
///
/// All variants are synchronous — the difference is only in how nodes within
/// the same topological level are scheduled.
///
/// `Parallel` is the recommended default on native targets. On targets where
/// rayon's worker pool is not initialised (notably wasm without
/// `wasm-bindgen-rayon` setup), rayon transparently falls back to sequential
/// execution, so `Parallel` is safe everywhere.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Execute nodes one at a time in stable topological order.
    Sequential,
    /// Execute same-level nodes in parallel via rayon (best-effort).
    #[default]
    Parallel,
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

/// Read-only queries on a node graph.
pub trait NodeGraphRead {
    fn get_node_by_id(&self, node_id: &NodeId) -> Option<NodeEntity>;
    fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId>;
    fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)>;
    fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<Data>;
    fn node_variants(&self) -> &HashSet<String>;
    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, String>;
}

/// Mutation methods on a node graph.
pub trait NodeGraphWrite {
    fn create_node<T: NodeImpl + NodeMeta + 'static>(&mut self) -> Result<NodeId, String>;
    fn create_node_with_id<T: NodeImpl + NodeMeta + 'static>(
        &mut self,
        id: NodeId,
    ) -> Result<NodeId, String>;
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
    fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String>;
    fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), String>;
}

impl NodeGraphRead for NodeGraph {
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

    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, String> {
        let ns = self.node_states.read().map_err(|e| e.to_string())?;
        ns.get_edge(&edge_id)
            .cloned()
            .ok_or(format!("Edge not found: id {:?}", edge_id))
    }
}

impl NodeGraphWrite for NodeGraph {
    fn create_node<T: NodeImpl + NodeMeta + 'static>(&mut self) -> Result<NodeId, String> {
        let node_id = self.node_manager.create_node::<T>()?;

        // Register in NodeStates
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_node(
                node_id,
                T::NAME,
                Some(TypeId::of::<T>()),
                T::DEFAULT_VALUE.to_data(),
                T::INPUTS,
                T::OUTPUTS,
            );
        }

        Ok(node_id)
    }

    fn create_node_with_id<T: NodeImpl + NodeMeta + 'static>(
        &mut self,
        id: NodeId,
    ) -> Result<NodeId, String> {
        let node_id = self.node_manager.create_node_with_id::<T>(id)?;

        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_node(
                node_id,
                T::NAME,
                Some(TypeId::of::<T>()),
                T::DEFAULT_VALUE.to_data(),
                T::INPUTS,
                T::OUTPUTS,
            );
        }

        Ok(node_id)
    }

    fn create_node_by_name(&mut self, name: &str) -> Result<NodeId, String> {
        // Create node and get default data from factory
        let (node_id, default_data, type_id) = self.node_manager.create_node_by_name(name)?;

        // Get type info for slot counts
        let type_info = crate::get_node_type_info(name)
            .ok_or_else(|| format!("NodeTypeInfo not found for '{}'", name))?;

        // Register in NodeStates
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_node(
                node_id,
                type_info.name,
                type_id,
                default_data,
                type_info.inputs,
                type_info.outputs,
            );
        }

        Ok(node_id)
    }

    fn create_node_by_name_with_id(&mut self, id: NodeId, name: &str) -> Result<NodeId, String> {
        let (_node_id, default_data, type_id) =
            self.node_manager.create_node_by_name_with_id(id, name)?;

        let type_info = crate::get_node_type_info(name)
            .ok_or_else(|| format!("NodeTypeInfo not found for '{}'", name))?;

        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_node(
                id,
                type_info.name,
                type_id,
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
}

/// Execute a single node synchronously.
///
/// Locks the nodes map only long enough to clone out the node entity, then
/// holds a read lock on the entity for regular nodes (allowing concurrent
/// reads from UI) and upgrades to a write lock for `SubGraphNode` (which
/// needs `&mut self` to drive its internal graph).
///
/// Wraps execution in `catch_unwind` so a single node panic does not abort
/// the surrounding rayon level.
fn execute_node_sync(
    node_id: NodeId,
    shared_nodes: &crate::SharedNodes,
    shared_cache: &SharedExecutionCache,
    shared_node_states: &SharedNodeStates,
    on_progress: &(dyn Fn(ExecutionEvent) + Send + Sync),
) -> (NodeId, Result<(), String>) {
    on_progress(ExecutionEvent::Started(node_id));

    let result: Result<(), String> = {
        let entity = {
            let nodes_guard = match shared_nodes.lock() {
                Ok(g) => g,
                Err(_) => {
                    on_progress(ExecutionEvent::Failed(node_id, "Lock poisoned".to_string()));
                    return (node_id, Err("Lock poisoned".to_string()));
                }
            };
            nodes_guard.get(&node_id).map(Arc::clone)
        };

        let Some(node_entity) = entity else {
            on_progress(ExecutionEvent::Completed(node_id));
            return (node_id, Ok(()));
        };

        // Identify SubGraphNode under a read lock first to decide whether we
        // need the write lock. Most nodes are regular and never need it.
        let is_subgraph = match node_entity.read() {
            Ok(r) => r.as_any().downcast_ref::<SubGraphNode>().is_some(),
            Err(poisoned) => poisoned
                .into_inner()
                .as_any()
                .downcast_ref::<SubGraphNode>()
                .is_some(),
        };

        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if is_subgraph {
                let mut node_write = match node_entity.write() {
                    Ok(w) => w,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let sg = node_write
                    .as_any_mut()
                    .downcast_mut::<SubGraphNode>()
                    .expect("downcast verified above");
                sg.execute_internal_sync(&node_id, shared_cache.share(), shared_node_states.clone())
            } else {
                let node_read = match node_entity.read() {
                    Ok(r) => r,
                    Err(poisoned) => poisoned.into_inner(),
                };
                match build_execution_context(&node_id, shared_node_states, shared_cache) {
                    Ok(ctx) => node_read.execute(ctx),
                    Err(e) => Err(e),
                }
            }
        }));

        match panic_result {
            Ok(res) => res,
            Err(panic_payload) => {
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    format!("Node panicked: {s}")
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    format!("Node panicked: {s}")
                } else {
                    "Node panicked".to_string()
                };
                Err(msg)
            }
        }
    };

    match &result {
        Ok(()) => on_progress(ExecutionEvent::Completed(node_id)),
        Err(msg) => on_progress(ExecutionEvent::Failed(node_id, msg.clone())),
    }

    (node_id, result)
}

impl NodeGraph {
    /// Execute the graph synchronously with the default mode (`Parallel`).
    ///
    /// This is the canonical execution kernel. It does not depend on any
    /// async runtime — callers may invoke it from event handlers, reactive
    /// effects, or async test contexts without nested executor coordination.
    ///
    /// Returns `ExecutionResult` containing outputs from computed nodes,
    /// edge values, execution errors, and IDs of removed nodes.
    pub fn execute_sync(&self) -> Result<ExecutionResult, String> {
        self.execute_sync_with_progress(ExecutionMode::default(), |_| {})
    }

    /// Execute the graph synchronously with the requested scheduling mode.
    pub fn execute_sync_with_mode(&self, mode: ExecutionMode) -> Result<ExecutionResult, String> {
        self.execute_sync_with_progress(mode, |_| {})
    }

    /// Execute the graph synchronously with the requested scheduling mode
    /// and a progress callback that fires per node as it starts, completes,
    /// or fails.
    pub fn execute_sync_with_progress<F>(
        &self,
        mode: ExecutionMode,
        on_progress: F,
    ) -> Result<ExecutionResult, String>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static,
    {
        let on_progress: Arc<dyn Fn(ExecutionEvent) + Send + Sync> = Arc::new(on_progress);
        self.execute_sync_inner(mode, on_progress)
    }

    /// Backward-compatible async facade. Returns the same value as
    /// [`execute_sync`]; the future is immediately ready and is not bound
    /// to any async runtime.
    pub async fn execute(&self) -> Result<ExecutionResult, String> {
        self.execute_sync()
    }

    /// Backward-compatible async facade for [`execute_sync_with_progress`]
    /// with `ExecutionMode::Parallel`.
    pub async fn execute_with_progress<F>(&self, on_progress: F) -> Result<ExecutionResult, String>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static,
    {
        self.execute_sync_with_progress(ExecutionMode::default(), on_progress)
    }

    fn execute_sync_inner(
        &self,
        mode: ExecutionMode,
        on_progress: Arc<dyn Fn(ExecutionEvent) + Send + Sync>,
    ) -> Result<ExecutionResult, String> {
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
            "[cognet] execute_sync: changed={} removed={} mode={:?}",
            changed.len(),
            removed.len(),
            mode,
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

        for level_nodes in sorted_node_levels {
            let level_results: Vec<(NodeId, Result<(), String>)> = match mode {
                ExecutionMode::Sequential => level_nodes
                    .into_iter()
                    .map(|node_id| {
                        execute_node_sync(
                            node_id,
                            &shared_nodes,
                            &shared_cache,
                            &shared_node_states,
                            on_progress.as_ref(),
                        )
                    })
                    .collect(),
                ExecutionMode::Parallel => level_nodes
                    .into_par_iter()
                    .map(|node_id| {
                        execute_node_sync(
                            node_id,
                            &shared_nodes,
                            &shared_cache,
                            &shared_node_states,
                            on_progress.as_ref(),
                        )
                    })
                    .collect(),
            };

            for (node_id, res) in level_results {
                if let Err(msg) = res {
                    self.add_error(GraphError::execution(node_id, msg));
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
}

#[cfg(test)]
mod sync_executor_tests {
    use super::*;
    use crate::{AddNode, NumberNode, SubGraphNode};

    /// Build a small `(a + b)` graph and return its three node ids.
    fn build_add_graph(a: f64, b: f64) -> (NodeGraph, NodeId, NodeId, NodeId) {
        let mut graph = NodeGraph::new().unwrap();
        let a_id = graph.create_node::<NumberNode>().unwrap();
        let b_id = graph.create_node::<NumberNode>().unwrap();
        let add_id = graph.create_node::<AddNode>().unwrap();

        graph
            .update_node_data(&a_id, Data::new(a).unwrap())
            .unwrap();
        graph
            .update_node_data(&b_id, Data::new(b).unwrap())
            .unwrap();
        graph.connect_nodes(&a_id, 0, &add_id, 0).unwrap();
        graph.connect_nodes(&b_id, 0, &add_id, 1).unwrap();

        (graph, a_id, b_id, add_id)
    }

    fn output_for(result: &ExecutionResult, node_id: NodeId) -> f64 {
        result
            .node_outputs
            .get(&node_id)
            .and_then(|slots| slots.first().cloned().flatten())
            .and_then(|d| d.value::<f64>().ok().copied())
            .expect("node output present")
    }

    #[test]
    fn sequential_matches_parallel_on_independent_levels() {
        // Two parallel sub-additions feed into a final add.
        let mut graph = NodeGraph::new().unwrap();
        let n1 = graph.create_node::<NumberNode>().unwrap();
        let n2 = graph.create_node::<NumberNode>().unwrap();
        let n3 = graph.create_node::<NumberNode>().unwrap();
        let n4 = graph.create_node::<NumberNode>().unwrap();
        let add_left = graph.create_node::<AddNode>().unwrap();
        let add_right = graph.create_node::<AddNode>().unwrap();
        let add_top = graph.create_node::<AddNode>().unwrap();

        graph
            .update_node_data(&n1, Data::new(1.0_f64).unwrap())
            .unwrap();
        graph
            .update_node_data(&n2, Data::new(2.0_f64).unwrap())
            .unwrap();
        graph
            .update_node_data(&n3, Data::new(3.0_f64).unwrap())
            .unwrap();
        graph
            .update_node_data(&n4, Data::new(4.0_f64).unwrap())
            .unwrap();
        graph.connect_nodes(&n1, 0, &add_left, 0).unwrap();
        graph.connect_nodes(&n2, 0, &add_left, 1).unwrap();
        graph.connect_nodes(&n3, 0, &add_right, 0).unwrap();
        graph.connect_nodes(&n4, 0, &add_right, 1).unwrap();
        graph.connect_nodes(&add_left, 0, &add_top, 0).unwrap();
        graph.connect_nodes(&add_right, 0, &add_top, 1).unwrap();

        let seq = graph
            .execute_sync_with_mode(ExecutionMode::Sequential)
            .unwrap();
        let seq_top = output_for(&seq, add_top);

        // Mark all dirty so a second execute recomputes the same nodes.
        graph.mark_all_nodes_dirty();
        let par = graph
            .execute_sync_with_mode(ExecutionMode::Parallel)
            .unwrap();
        let par_top = output_for(&par, add_top);

        assert_eq!(seq_top, 10.0);
        assert_eq!(par_top, 10.0);
    }

    #[test]
    fn execute_sync_produces_expected_value() {
        let (graph, _, _, add_id) = build_add_graph(7.0, 8.0);
        let result = graph.execute_sync().unwrap();
        assert_eq!(output_for(&result, add_id), 15.0);
    }

    #[test]
    fn async_execute_facade_returns_same_value() {
        // The async execute() is a thin wrapper that does not require any
        // runtime — `block_on` from `pollster` would also work, but since the
        // future is immediately ready we can poll it inline with a noop waker.
        use std::future::Future;
        use std::pin::Pin;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);

        let (graph, _, _, add_id) = build_add_graph(2.5, 3.5);
        let mut fut = Box::pin(graph.execute());
        let result = match Pin::new(&mut fut).poll(&mut cx) {
            Poll::Ready(r) => r.unwrap(),
            Poll::Pending => panic!("compat facade must be immediately ready"),
        };
        assert_eq!(output_for(&result, add_id), 6.0);
    }

    #[test]
    fn subgraph_executes_recursively_through_sync_kernel() {
        let mut graph = NodeGraph::new().unwrap();
        let outer_a = graph.create_node::<NumberNode>().unwrap();
        let outer_b = graph.create_node::<NumberNode>().unwrap();
        graph
            .update_node_data(&outer_a, Data::new(11.0_f64).unwrap())
            .unwrap();
        graph
            .update_node_data(&outer_b, Data::new(31.0_f64).unwrap())
            .unwrap();

        // Build a subgraph that sums two inputs.
        let sub_id = graph.create_node::<SubGraphNode>().unwrap();
        graph
            .add_subgraph_input(&sub_id, "A", DataType::Number)
            .unwrap();
        graph
            .add_subgraph_input(&sub_id, "B", DataType::Number)
            .unwrap();
        graph
            .add_subgraph_output(&sub_id, "Sum", DataType::Number)
            .unwrap();

        let (in_proxy, out_proxy) = graph.subgraph_proxy_ids(&sub_id).unwrap();
        let inner_add = graph
            .with_subgraph_mut(&sub_id, |internal| {
                let inner_add = internal.create_node::<AddNode>().unwrap();
                internal.connect_nodes(&in_proxy, 0, &inner_add, 0).unwrap();
                internal.connect_nodes(&in_proxy, 1, &inner_add, 1).unwrap();
                internal
                    .connect_nodes(&inner_add, 0, &out_proxy, 0)
                    .unwrap();
                inner_add
            })
            .unwrap();
        let _ = inner_add; // value unused after construction; only for clarity

        graph.connect_nodes(&outer_a, 0, &sub_id, 0).unwrap();
        graph.connect_nodes(&outer_b, 0, &sub_id, 1).unwrap();

        let result = graph.execute_sync().unwrap();
        assert_eq!(output_for(&result, sub_id), 42.0);
    }
}
