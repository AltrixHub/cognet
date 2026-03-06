#[cfg(not(target_arch = "wasm32"))]
use tokio::{runtime::Handle, task};

#[cfg(target_arch = "wasm32")]
use futures::future::join_all;

use async_trait::async_trait;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::{collections::HashSet, sync::Arc};

use crate::{
    node_graph_system::NodeGraphSystem, Data, Edge, EdgeId, ExecutionContext, GraphError,
    NodeEntity, NodeGraph, NodeId, NodeImpl, NodeMeta, OutputWriter, SharedExecutionCache,
    SharedNodeStates, SubGraphNode,
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

/// Changes produced by a graph execution.
///
/// Contains all outputs from nodes that were (re)computed, plus any nodes
/// that were removed since the last execution.
#[derive(Debug, Clone, Default)]
pub struct GraphChanges {
    /// Outputs from nodes computed in this execution (new + updated).
    pub outputs: Vec<NodeOutput>,
    /// Nodes removed since the last execute() call.
    pub removed_nodes: Vec<NodeId>,
}

impl GraphChanges {
    /// Create empty changes.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty() && self.removed_nodes.is_empty()
    }
}

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
        .filter_map(|idx| {
            ns.output_slot(node_id, idx)
                .map(|s| (s.id, s.data_type))
        })
        .collect();
    let output_writer = OutputWriter::new(cache.share(), slots);

    Ok(ExecutionContext {
        node_data,
        input_values,
        output_writer,
    })
}

#[async_trait]
pub trait NodeGraphAPI {
    fn new() -> Result<Self, String>
    where
        Self: Sized;

    /// Execute the graph asynchronously.
    ///
    /// Returns `GraphChanges` containing outputs from computed nodes
    /// and IDs of nodes removed since the last execution.
    async fn execute(&self) -> Result<GraphChanges, String>;

    /// Execute the graph with progress callback.
    ///
    /// The callback is called for each node as it starts executing,
    /// completes, or fails. This enables real-time UI updates.
    ///
    /// Returns `GraphChanges` containing outputs from computed nodes
    /// and IDs of nodes removed since the last execution.
    async fn execute_with_progress<F>(&self, on_progress: F) -> Result<GraphChanges, String>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static;

    // Sync methods for graph structure operations
    fn create_node<T: NodeImpl + NodeMeta + 'static>(&mut self) -> Result<NodeId, String>;
    fn create_node_by_name(&mut self, name: &str) -> Result<NodeId, String>;
    fn remove_node(&mut self, node_id: NodeId) -> Result<(), String>;
    fn update_node_data(&mut self, node_id: &NodeId, data: Data) -> Result<(), String>;
    fn update_input_slot_default_data(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        data: Data,
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

    async fn execute(&self) -> Result<GraphChanges, String> {
        self.execute_with_progress(|_| {}).await
    }

    async fn execute_with_progress<F>(&self, on_progress: F) -> Result<GraphChanges, String>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static,
    {
        // Drain bookkeeping state atomically
        let (dirty_nodes, removed) = {
            let mut b = self
                .bookkeeping
                .lock()
                .map_err(|e| e.to_string())?;
            let dirty = std::mem::take(&mut b.dirty_nodes);
            let removed = std::mem::take(&mut b.removed_since_last_execute);
            (dirty, removed)
        };

        tracing::debug!(
            target: "graph",
            "[cognet] execute: dirty={} removed={}",
            dirty_nodes.len(),
            removed.len(),
        );

        if dirty_nodes.is_empty() && removed.is_empty() {
            return Ok(GraphChanges::empty());
        }

        // If only removals (no dirty nodes), return just the removed list
        if dirty_nodes.is_empty() {
            return Ok(GraphChanges {
                outputs: Vec::new(),
                removed_nodes: removed,
            });
        }

        // Clear previous execution errors before running
        self.clear_execution_errors();

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
                                    if let Some(sg) = node_read
                                        .as_any()
                                        .downcast_ref::<SubGraphNode>()
                                    {
                                        sg.execute_internal(
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
                                        Err(_) => return (node_id, Err("Lock poisoned".to_string())),
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
                                                rt_clone.block_on(async {
                                                    // Read lock only — execute() and
                                                    // execute_internal() take &self.
                                                    let node_read = match node_clone.read() {
                                                        Ok(r) => r,
                                                        Err(poisoned) => poisoned.into_inner(),
                                                    };
                                                    if let Some(sg) = node_read
                                                        .as_any()
                                                        .downcast_ref::<SubGraphNode>()
                                                    {
                                                        sg.execute_internal(
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
                                                            Ok(ctx) => {
                                                                node_read.execute(ctx).await
                                                            }
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
        if let Ok(ns) = shared_node_states.read() {
            if let Ok(cache) = shared_cache.read() {
                for node_id in &executed_node_ids {
                    let output_count = ns.output_slot_count(node_id);
                    for idx in 0..output_count {
                        if let Some(slot) = ns.output_slot(node_id, idx) {
                            if let Some(data) = cache.outputs.get(&slot.id) {
                                outputs.push(NodeOutput {
                                    node_id: *node_id,
                                    slot_index: idx,
                                    data: data.share(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(GraphChanges {
            outputs,
            removed_nodes: removed,
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

    fn remove_node(&mut self, node_id: NodeId) -> Result<(), String> {
        if self.node_manager.node_remove(&node_id).is_some() {
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id])?;
            self.remove_edges_from_cache(&node_id)?;
            self.mark_dirty_nodes(dirty_nodes);

            // Clean up NodeStates
            {
                let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
                guard.remove_node(&node_id);
            }

            // Track removal for GraphChanges in next execute()
            self.record_removal(node_id);

            Ok(())
        } else {
            Err("Node not found.".to_string())
        }
    }

    fn update_node_data(&mut self, node_id: &NodeId, data: Data) -> Result<(), String> {
        // Write to NodeStates (source of truth)
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            let node_state = guard
                .get_mut(node_id)
                .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;
            node_state.data = Some(data.share());
        }

        // Also update NodeEntity (backward compatibility for SubGraphNode)
        if let Some(node) = self.node_manager.get_node_by_id(node_id) {
            if let Ok(mut write_node) = node.write() {
                let _ = write_node.set_node_data(data);
            }
        }

        let dirty_nodes = self.collect_dirty_nodes(vec![*node_id])?;
        self.mark_dirty_nodes(dirty_nodes);
        Ok(())
    }

    fn update_input_slot_default_data(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String> {
        // Write to NodeStates (source of truth)
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            let slot = guard
                .input_slot_mut(node_id, slot_index)
                .ok_or_else(|| format!("Input slot {} not found for node {:?}", slot_index, node_id))?;
            slot.default_value = Some(data.share().into_value());
        }

        // Also update NodeEntity (backward compatibility for SubGraphNode)
        if let Some(node) = self.node_manager.get_node_by_id(node_id) {
            if let Ok(mut write_node) = node.write() {
                let _ = write_node.set_input_slot_default_data(slot_index, data);
            }
        }

        let dirty_nodes = self.collect_dirty_nodes(vec![*node_id])?;
        self.mark_dirty_nodes(dirty_nodes);
        Ok(())
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
        // remove_edge_from_cache now removes from NodeStates
        // (including slot connected_edges and all indexes)
        let edge = self.remove_edge_from_cache(&edge_id)?;

        let dirty_nodes =
            self.collect_dirty_nodes(vec![edge.from_node_id, edge.to_node_id])?;
        self.mark_dirty_nodes(dirty_nodes);

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
