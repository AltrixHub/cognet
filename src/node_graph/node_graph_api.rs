use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    sync::Arc,
};

use std::any::TypeId;

use crate::{
    node_graph_system::NodeGraphSystem, BoxNodeFuture, ColorValue, Data, DataType, DataValue, Edge,
    EdgeId, ErrorTarget, ExecutionContext, GraphError, NodeEntity, NodeExecutionKind, NodeGraph,
    NodeId, NodeImpl, NodeMeta, NodePath, OutputWriter, SharedExecutionCache, SharedNodeStates,
    Vector3,
};

/// Typed error returned by `NodeGraph::execute_sync` and `execute_async`.
///
/// `RequiresAsyncExecution` is the load-bearing variant: it lets a sync
/// caller distinguish "this graph contains async I/O nodes — call
/// `execute_async`" from genuine planning or execution failure. Per
/// plan-14c the dirty-state contract requires that returning this error
/// must NOT clear the changed-node set, so a follow-up `execute_async`
/// can run the same plan.
///
/// plan-006 P3c: `RequiresAsyncSubgraphSupport` is deleted — with
/// transparent SubGraphs, every node in the plan executes at the same
/// level and there is no opaque internal graph to check separately.
#[derive(Debug, Clone)]
pub enum GraphExecutionError {
    /// The dirty plan contains one or more `AsyncIo` nodes; the sync
    /// kernel cannot run them. Re-run via `execute_async`. Dirty state
    /// is preserved so the async re-run sees the same plan.
    RequiresAsyncExecution { node_ids: Vec<NodeId> },
    /// The execution plan could not be built (e.g. lock poisoning,
    /// topological cycle, missing graph metadata).
    PlanningFailed(String),
    /// One of the node executions failed in a way the planner cannot
    /// retry from. Per-node execution failures are recorded separately
    /// inside `ExecutionResult` and do NOT raise this variant.
    ExecutionFailed(String),
}

impl fmt::Display for GraphExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequiresAsyncExecution { node_ids } => write!(
                f,
                "graph contains {} AsyncIo node(s); call `execute_async` instead",
                node_ids.len()
            ),
            Self::PlanningFailed(msg) => write!(f, "graph planning failed: {msg}"),
            Self::ExecutionFailed(msg) => write!(f, "graph execution failed: {msg}"),
        }
    }
}

impl std::error::Error for GraphExecutionError {}

/// Topologically sorted execution plan plus the dirty set it was built
/// from. The planner snapshots the changed-node set without draining
/// it, so a `RequiresAsyncExecution` rejection leaves the graph
/// re-executable from the same dirty state.
struct ExecutionPlan {
    /// Snapshot of node paths that were dirty when the plan was built.
    /// The executor drains the underlying `NodeStates::changed_nodes`
    /// only after API validation succeeds.
    dirty_nodes: HashSet<NodePath>,
    /// Topologically grouped node levels — same-level nodes have no
    /// data dependencies on each other and may run in parallel.
    levels: Vec<Vec<NodePath>>,
    /// Flattened executed-node list in topological order, for output
    /// collection.
    executed_node_paths: Vec<NodePath>,
    /// Removed nodes drained from bookkeeping at plan time (they don't
    /// participate in execution but are reported in `ExecutionResult`).
    /// Removal bookkeeping is id-keyed (the Remove API takes a
    /// `NodeId`), so this list stays as `NodeId`.
    removed_nodes: Vec<NodeId>,
}

impl ExecutionPlan {
    fn is_empty(&self) -> bool {
        self.executed_node_paths.is_empty()
    }
}

/// Execution event for progress tracking during graph execution.
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Node execution started.
    Started(NodePath),
    /// Node execution completed successfully.
    Completed(NodePath),
    /// Node execution failed with an error.
    Failed(NodePath, String),
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

/// Value flowing through an edge after execution.
///
/// plan-007 keeps `from_node` / `to_node` as `NodeId` for now — the
/// edge-value scope is intentionally out of the path migration; the
/// executor wraps depth via `.leaf()` at the single collection site.
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
/// Contains all node outputs keyed by `NodePath`, edge values, execution
/// errors keyed by `NodePath`, and removed nodes (still id-keyed —
/// removal bookkeeping is `NodeId`-driven).
#[derive(Debug, Clone, Default)]
pub struct ExecutionResult {
    /// Outputs from nodes computed in this execution (new + updated).
    pub node_outputs: HashMap<NodePath, Vec<Option<Data>>>,
    /// Values flowing through edges after execution.
    pub edge_values: HashMap<EdgeId, EdgeValue>,
    /// Execution errors per node path.
    pub errors: HashMap<NodePath, NodeError>,
    /// Nodes removed since the last execute() call.
    pub removed_nodes: Vec<NodeId>,
}

impl ExecutionResult {
    /// Create empty result.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.node_outputs.is_empty() && self.removed_nodes.is_empty()
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
    path: &NodePath,
    node_states: &SharedNodeStates,
    cache: &SharedExecutionCache,
) -> Result<ExecutionContext, String> {
    let ns = node_states.read().map_err(|e| e.to_string())?;
    let cache_read = cache.read()?;

    // Resolve input values from NodeStates slot metadata
    let input_count = ns.input_slot_count(path);
    let input_values = (0..input_count)
        .map(|idx| {
            let mut values = Vec::new();
            if let Some(slot_state) = ns.input_slot(path, idx) {
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
        .get(path)
        .and_then(|state| state.data.as_ref().map(|d| d.share()));

    // Build output writer from NodeStates output slot metadata
    let output_count = ns.output_slot_count(path);
    let slots = (0..output_count)
        .filter_map(|idx| ns.output_slot(path, idx).map(|s| (s.id, s.data_type)))
        .collect();
    let output_writer = OutputWriter::new(cache.share(), slots);

    Ok(ExecutionContext {
        node_data,
        input_values,
        output_writer,
    })
}

/// Extract field name/value pairs from a `Data` value for composite types.
pub(super) fn extract_fields_from_data(
    data_type: DataType,
    data: Option<&Data>,
) -> Vec<(&'static str, f64)> {
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
pub(super) fn extract_fields_from_default_value(
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
    fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId> {
        self.node_manager.get_node_ids_by_type::<T>()
    }

    fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)> {
        self.node_manager.get_nodes_by_ids(ids)
    }

    fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<Data> {
        let ns = self.node_states.read().ok()?;
        let slot = ns.output_slot(&NodePath::root().child(*node_id), output_slot_index)?;
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
                &NodePath::root().child(node_id),
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
                &NodePath::root().child(node_id),
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
        self.create_node_by_name_at(&NodePath::root(), name)
    }

    fn create_node_by_name_with_id(&mut self, id: NodeId, name: &str) -> Result<NodeId, String> {
        self.create_node_by_name_at_with_id(&NodePath::root(), id, name)
    }

    fn remove_node(&mut self, node_id: NodeId) -> Result<(), String> {
        if self.node_manager.node_remove(&node_id).is_some() {
            {
                let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
                guard.remove_node(&NodePath::root().child(node_id)); // remove_edge auto-tracks downstream
            }
            self.record_removal(node_id);
            Ok(())
        } else {
            Err("Node not found.".to_string())
        }
    }

    fn update_node_data(&mut self, node_id: &NodeId, data: Data) -> Result<(), String> {
        self.update_node_data_at(&NodePath::root().child(*node_id), data)
    }

    fn update_input_slot_default_data(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String> {
        self.update_input_slot_default_data_at(&NodePath::root().child(*node_id), slot_index, data)
    }

    fn update_node_data_field(
        &mut self,
        node_id: &NodeId,
        data_type: DataType,
        field_name: &str,
        value: f64,
    ) -> Result<(), String> {
        self.update_node_data_field_at(
            &NodePath::root().child(*node_id),
            data_type,
            field_name,
            value,
        )
    }

    fn update_input_slot_default_field(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        field_name: &str,
        value: f64,
    ) -> Result<(), String> {
        self.update_input_slot_default_field_at(
            &NodePath::root().child(*node_id),
            slot_index,
            field_name,
            value,
        )
    }

    fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String> {
        self.connect_nodes_at(
            &NodePath::root().child(*from_node_id),
            from_output_slot_index,
            &NodePath::root().child(*to_node_id),
            to_input_slot_index,
        )
    }

    fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), String> {
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.remove_edge(&edge_id)
            .ok_or(format!("Edge does not exist: id: {:?}", edge_id))?;
        Ok(())
    }
}

/// Look up a node's execution kind (`SyncCpu` / `AsyncIo`) by `NodePath`.
fn node_execution_kind(
    path: &NodePath,
    shared_nodes: &crate::SharedNodes,
) -> Option<NodeExecutionKind> {
    let entity = {
        let guard = shared_nodes.lock().ok()?;
        guard.get(path).map(Arc::clone)?
    };
    let read = match entity.read() {
        Ok(r) => r,
        Err(poisoned) => poisoned.into_inner(),
    };
    Some(read.execution_kind())
}

/// Validate that no node in the sync plan requires `AsyncIo` execution.
///
/// Replaces the old `validate_async_subgraphs` + `subgraph_internal_has_async`
/// pair (plan-006 P3c Step 11.3). With a transparent SubGraph boundary, every
/// node in the execution plan is a direct entry — no downcast or internal-graph
/// recursion needed. This is a flat check over the executed `NodePath` list.
///
/// The `RequiresAsyncExecution` error variant still reports `Vec<NodeId>` so
/// it stays compatible with id-keyed callers; depth-2 async nodes are
/// projected via `.leaf()` at the error boundary.
fn validate_async_in_sync_plan(
    executed_paths: &[NodePath],
    shared_nodes: &crate::SharedNodes,
) -> Result<(), GraphExecutionError> {
    let async_nodes: Vec<NodeId> = executed_paths
        .iter()
        .filter(|path| {
            node_execution_kind(path, shared_nodes)
                .map(|k| k == NodeExecutionKind::AsyncIo)
                .unwrap_or(false)
        })
        .filter_map(|path| path.leaf())
        .collect();
    if async_nodes.is_empty() {
        Ok(())
    } else {
        Err(GraphExecutionError::RequiresAsyncExecution {
            node_ids: async_nodes,
        })
    }
}

/// Execute a single node synchronously.
///
/// Locks the nodes map only long enough to clone out the node entity, then
/// holds a read lock on the entity while building the execution context and
/// dispatching `execute_sync`. The transparent SubGraph boundary (plan-006
/// P3c Step 11.1) means there is no special-casing for SubGraphNode — it
/// simply calls `execute_sync` like every other node, and its children are
/// visited directly by the planner.
///
/// Wraps execution in `catch_unwind` so a single node panic does not abort
/// the surrounding rayon level.
fn execute_node_sync(
    path: NodePath,
    shared_nodes: &crate::SharedNodes,
    shared_cache: &SharedExecutionCache,
    shared_node_states: &SharedNodeStates,
    on_progress: &(dyn Fn(ExecutionEvent) + Send + Sync),
) -> (NodePath, Result<(), String>) {
    on_progress(ExecutionEvent::Started(path.clone()));

    let result: Result<(), String> = {
        let entity = {
            let nodes_guard = match shared_nodes.lock() {
                Ok(g) => g,
                Err(_) => {
                    on_progress(ExecutionEvent::Failed(
                        path.clone(),
                        "Lock poisoned".to_string(),
                    ));
                    return (path, Err("Lock poisoned".to_string()));
                }
            };
            nodes_guard.get(&path).map(Arc::clone)
        };

        let Some(node_entity) = entity else {
            on_progress(ExecutionEvent::Completed(path.clone()));
            return (path, Ok(()));
        };

        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let node_read = match node_entity.read() {
                Ok(r) => r,
                Err(poisoned) => poisoned.into_inner(),
            };
            match build_execution_context(&path, shared_node_states, shared_cache) {
                Ok(ctx) => node_read.execute_sync(ctx),
                Err(e) => Err(e),
            }
            // node_read drops at end of scope — locks released
            // before the next level starts.
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
        Ok(()) => on_progress(ExecutionEvent::Completed(path.clone())),
        Err(msg) => on_progress(ExecutionEvent::Failed(path.clone(), msg.clone())),
    }

    (path, result)
}

impl NodeGraph {
    // === Runtime factory passthroughs (pillar-2 BIM templates) ===========

    /// Register a runtime-named factory on this graph's NodeManager so
    /// nodes of that name can be created via `create_node_by_name` /
    /// `create_node_by_name_with_id`. Mirrors
    /// [`NodeManager::register_factory_with_name_owned`] so callers
    /// that only have access to a `NodeGraph` (not the wrapped
    /// `NodeManager`) can install per-instance factories.
    pub fn register_factory_with_name_owned(
        &mut self,
        name: String,
        factory: Arc<dyn Fn() -> Result<NodeEntity, String> + Send + Sync>,
        default_data: Option<Data>,
        type_id: Option<TypeId>,
    ) {
        self.node_manager
            .register_factory_with_name_owned(name, factory, default_data, type_id);
    }

    /// Remove a runtime-named factory registration. Returns the
    /// previous entry if any. Pass-through to
    /// [`NodeManager::unregister_factory_by_name`].
    pub fn unregister_factory_by_name(&mut self, name: &str) -> Option<crate::NodeFactoryWithMeta> {
        self.node_manager.unregister_factory_by_name(name)
    }

    /// Re-insert a previously-removed factory registration under the
    /// same (already-leaked) name. Pass-through to
    /// [`NodeManager::restore_factory_registration`].
    pub fn restore_factory_registration(
        &mut self,
        name: &str,
        entry: crate::NodeFactoryWithMeta,
    ) -> Result<(), crate::NodeFactoryWithMeta> {
        self.node_manager.restore_factory_registration(name, entry)
    }

    // === Graph-shaped factories (plan-007 P007b) =========================

    /// Register a graph-shaped factory (plan-007 P007b).
    ///
    /// Unlike [`register_factory_with_name_owned`] which dispenses
    /// single `NodeEntity` values, a [`GraphFactory`] is a closure that
    /// drives the graph itself — it can call `add_subgraph_at*`,
    /// `create_node_by_name_at`, `connect_nodes_at`, and even other
    /// `create_graph_by_name_at` calls to build a composite of nodes
    /// and edges under a caller-supplied `parent_path`.
    ///
    /// Factories MAY recurse: the dispatcher
    /// ([`create_graph_by_name_at`]) clones the `Arc<dyn Fn>` and
    /// drops its borrow before invocation so nested factories run
    /// without lock contention.
    pub fn register_graph_factory(
        &mut self,
        name: impl Into<String>,
        factory: crate::GraphFactory,
    ) {
        self.node_manager.register_graph_factory(name, factory);
    }

    /// Remove a graph-shaped factory by name. Returns the previous
    /// entry if registered.
    pub fn unregister_graph_factory(&mut self, name: &str) -> Option<crate::GraphFactory> {
        self.node_manager.unregister_graph_factory(name)
    }

    /// Invoke the graph-shaped factory `name` to materialise a
    /// composite under `parent_path`.
    ///
    /// `id_hint` is forwarded to the factory; semantics are
    /// factory-specific (typical pattern: use it as the `NodeId` of
    /// the composite's top entity so callers can pre-reserve the id).
    /// Returns whatever `NodeId` the factory chose to expose as the
    /// composite's externally-visible root.
    pub fn create_graph_by_name_at(
        &mut self,
        parent_path: &NodePath,
        name: &str,
        id_hint: Option<NodeId>,
    ) -> Result<NodeId, String> {
        // Clone the Arc<dyn Fn> and drop the borrow so the factory may
        // re-enter `self` via further path-aware mutators (including
        // nested `create_graph_by_name_at` calls).
        let factory = self
            .node_manager
            .graph_factory(name)
            .ok_or_else(|| format!("Graph factory '{}' is not registered", name))?;
        (factory)(self, parent_path, id_hint)
    }

    // === Public synchronous API ===========================================

    /// Execute the dirty graph synchronously with the default scheduling
    /// mode (`Parallel`).
    ///
    /// Returns `RequiresAsyncExecution` (without losing dirty state) if
    /// the planned graph contains any `NodeExecutionKind::AsyncIo` nodes.
    /// In that case, call [`execute_async`](Self::execute_async).
    ///
    /// Per plan-14c the synchronous kernel never enters an async runtime
    /// and never calls `block_on`. It is safe to invoke from a UI event
    /// handler, a reactive effect, a Tokio task, or a sync test.
    pub fn execute_sync(&self) -> Result<ExecutionResult, GraphExecutionError> {
        self.execute_sync_with_progress(ExecutionMode::default(), |_| {})
    }

    /// Execute the dirty graph synchronously with the requested mode.
    pub fn execute_sync_with_mode(
        &self,
        mode: ExecutionMode,
    ) -> Result<ExecutionResult, GraphExecutionError> {
        self.execute_sync_with_progress(mode, |_| {})
    }

    /// Execute the dirty graph synchronously with the requested mode and
    /// a per-node progress callback.
    pub fn execute_sync_with_progress<F>(
        &self,
        mode: ExecutionMode,
        on_progress: F,
    ) -> Result<ExecutionResult, GraphExecutionError>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static,
    {
        let on_progress: Arc<dyn Fn(ExecutionEvent) + Send + Sync> = Arc::new(on_progress);
        self.execute_sync_inner(mode, on_progress)
    }

    // === Public asynchronous API ==========================================

    /// Execute the dirty graph asynchronously with the default scheduling
    /// mode (`Parallel`).
    ///
    /// Supports mixed `SyncCpu` + `AsyncIo` graphs. Sync nodes run on a
    /// CPU executor (rayon in `Parallel`, the calling thread in
    /// `Sequential`); async nodes return a `'static` future from
    /// `prepare_async` and are awaited concurrently within each
    /// topological level. Locks are dropped before any `.await`.
    pub async fn execute_async(&self) -> Result<ExecutionResult, GraphExecutionError> {
        self.execute_async_with_progress(ExecutionMode::default(), |_| {})
            .await
    }

    /// Async variant of [`execute_sync_with_mode`].
    pub async fn execute_async_with_mode(
        &self,
        mode: ExecutionMode,
    ) -> Result<ExecutionResult, GraphExecutionError> {
        self.execute_async_with_progress(mode, |_| {}).await
    }

    /// Async variant of [`execute_sync_with_progress`].
    pub async fn execute_async_with_progress<F>(
        &self,
        mode: ExecutionMode,
        on_progress: F,
    ) -> Result<ExecutionResult, GraphExecutionError>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static,
    {
        let on_progress: Arc<dyn Fn(ExecutionEvent) + Send + Sync> = Arc::new(on_progress);
        self.execute_async_inner(mode, on_progress).await
    }

    // === Backward-compatible async API ====================================

    /// Backward-compatible async entry point. Routes to the real async
    /// executor (not a sync wrapper). Existing call sites that write
    /// `graph.execute().await` continue to work.
    pub async fn execute(&self) -> Result<ExecutionResult, GraphExecutionError> {
        self.execute_async_with_mode(ExecutionMode::default()).await
    }

    /// Backward-compatible progress variant.
    pub async fn execute_with_progress<F>(
        &self,
        on_progress: F,
    ) -> Result<ExecutionResult, GraphExecutionError>
    where
        F: Fn(ExecutionEvent) + Send + Sync + 'static,
    {
        self.execute_async_with_progress(ExecutionMode::default(), on_progress)
            .await
    }

    // === Planning =========================================================

    /// Build the dirty execution plan WITHOUT clearing the changed-node
    /// set.
    ///
    /// Snapshots `NodeStates::changed_nodes` via peek (not drain), runs a
    /// BFS to collect downstream-affected nodes, and topologically sorts
    /// the result. Removed nodes are drained from bookkeeping at plan
    /// time — they don't participate in execution and are merely reported
    /// in `ExecutionResult`, so eager drainage is safe.
    fn build_execution_plan(&self) -> Result<ExecutionPlan, GraphExecutionError> {
        let removed = {
            let mut b = self
                .bookkeeping
                .lock()
                .map_err(|e| GraphExecutionError::PlanningFailed(e.to_string()))?;
            std::mem::take(&mut b.removed_since_last_execute)
        };

        let changed = {
            let ns = self
                .node_states
                .read()
                .map_err(|e| GraphExecutionError::PlanningFailed(e.to_string()))?;
            ns.peek_changed_nodes()
        };

        if changed.is_empty() {
            return Ok(ExecutionPlan {
                dirty_nodes: HashSet::new(),
                levels: Vec::new(),
                executed_node_paths: Vec::new(),
                removed_nodes: removed,
            });
        }

        let dirty_nodes: HashSet<NodePath> = {
            let ns = self
                .node_states
                .read()
                .map_err(|e| GraphExecutionError::PlanningFailed(e.to_string()))?;
            let mut affected: HashSet<NodePath> = HashSet::new();
            let mut queue: VecDeque<NodePath> = changed.into_iter().collect();
            while let Some(path) = queue.pop_front() {
                if affected.insert(path.clone()) {
                    for edge_id in ns.outgoing_edges_at(&path) {
                        if let Some(edge) = ns.get_edge(edge_id) {
                            if !affected.contains(&edge.to_node) {
                                queue.push_back(edge.to_node.clone());
                            }
                        }
                    }
                }
            }
            affected
        };

        let levels = self
            .topological_sort(&dirty_nodes)
            .map_err(GraphExecutionError::PlanningFailed)?;

        let executed_node_paths: Vec<NodePath> = levels
            .iter()
            .flat_map(|level| level.iter().cloned())
            .collect();

        Ok(ExecutionPlan {
            dirty_nodes,
            levels,
            executed_node_paths,
            removed_nodes: removed,
        })
    }

    /// Validate that the plan can run on the synchronous kernel.
    ///
    /// Returns `RequiresAsyncExecution` (without draining the dirty set) if
    /// any node in the plan has `NodeExecutionKind::AsyncIo`. The caller can
    /// then switch to `execute_async`.
    ///
    /// With transparent SubGraphs (plan-006 P3c) every node in the plan
    /// executes directly — there is no internal-graph recursion to inspect.
    fn validate_for_sync(&self, plan: &ExecutionPlan) -> Result<(), GraphExecutionError> {
        let shared_nodes = self.node_manager.nodes();
        validate_async_in_sync_plan(&plan.executed_node_paths, &shared_nodes)
    }

    /// Validate that the plan can run on the asynchronous kernel.
    ///
    /// With transparent SubGraphs (plan-006 P3c) every node in the plan
    /// executes directly — there is no internal graph to check separately.
    /// The async kernel handles `AsyncIo` nodes natively; no further
    /// validation is needed beyond what `build_execution_plan` already
    /// captured.
    fn validate_for_async(&self, _plan: &ExecutionPlan) -> Result<(), GraphExecutionError> {
        Ok(())
    }

    /// Drain the changed-nodes set after planning + validation succeeded.
    ///
    /// This is the moment the plan transitions from "tentative" to
    /// "in-flight": failures after this point are recorded as per-node
    /// execution errors inside the returned `ExecutionResult`, not as a
    /// graph-level `GraphExecutionError`.
    fn commit_plan_drain(&self) -> Result<(), GraphExecutionError> {
        let mut ns = self
            .node_states
            .write()
            .map_err(|e| GraphExecutionError::ExecutionFailed(e.to_string()))?;
        let _ = ns.drain_changed_nodes();
        Ok(())
    }

    fn empty_result_with_removed(plan: &ExecutionPlan) -> ExecutionResult {
        ExecutionResult {
            removed_nodes: plan.removed_nodes.clone(),
            ..Default::default()
        }
    }

    /// Collect outputs/edge_values/errors for the executed plan.
    fn collect_outputs(&self, plan: &ExecutionPlan) -> ExecutionResult {
        let shared_cache = self.cache.share();
        let shared_node_states = self.node_states.clone();
        let executed_node_paths = &plan.executed_node_paths;

        let mut node_outputs: HashMap<NodePath, Vec<Option<Data>>> = HashMap::new();
        let mut edge_values: HashMap<EdgeId, EdgeValue> = HashMap::new();

        if let Ok(ns) = shared_node_states.read() {
            if let Ok(cache) = shared_cache.read() {
                for node_path in executed_node_paths {
                    let output_count = ns.output_slot_count(node_path);
                    let mut slot_outputs = vec![None; output_count];
                    for (idx, slot_out) in slot_outputs.iter_mut().enumerate() {
                        if let Some(slot) = ns.output_slot(node_path, idx) {
                            if let Some(data) = cache.outputs.get(&slot.id) {
                                *slot_out = Some(data.share());
                            }
                        }
                    }

                    if output_count == 0 {
                        let input_count = ns.input_slot_count(node_path);
                        let mut slot_inputs = vec![None; input_count];
                        for edge in ns.edges().values() {
                            if edge.to_node == *node_path
                                && (edge.to_input_slot_index) < slot_inputs.len()
                            {
                                if let Some(data) = cache.outputs.get(&edge.from_output_slot_id) {
                                    slot_inputs[edge.to_input_slot_index] = Some(data.share());
                                }
                            }
                        }
                        node_outputs.insert(node_path.clone(), slot_inputs);
                    } else {
                        node_outputs.insert(node_path.clone(), slot_outputs);
                    }
                }

                // Walk only the edges fanning out from executed paths via
                // the reverse map, so we avoid an O(N x E) scan over the
                // full edge set. Mirrors the iteration shape used by
                // `topological_sort` in `node_graph_system.rs`.
                for path in executed_node_paths {
                    for edge_id in ns.outgoing_edges_at(path) {
                        let Some(edge) = ns.get_edge(edge_id) else {
                            continue;
                        };
                        let Some(from_id) = edge.from_node.leaf() else {
                            continue;
                        };
                        let Some(to_id) = edge.to_node.leaf() else {
                            continue;
                        };
                        let data = cache
                            .outputs
                            .get(&edge.from_output_slot_id)
                            .map(|d| d.share());
                        edge_values.insert(
                            *edge_id,
                            EdgeValue {
                                from_node: from_id,
                                from_slot: edge.from_output_slot_index,
                                to_node: to_id,
                                to_slot: edge.to_input_slot_index,
                                data,
                            },
                        );
                    }
                }
            }
        }

        let mut errors: HashMap<NodePath, NodeError> = HashMap::new();
        let all_errors = self.errors();
        for (target, err) in &all_errors {
            if let ErrorTarget::Node(node_id) = target {
                if err.is_execution_error() {
                    // plan-007: ErrorTarget migration TBD
                    errors.insert(
                        NodePath::root().child(*node_id),
                        NodeError {
                            message: err.message(),
                        },
                    );
                }
            }
        }

        ExecutionResult {
            node_outputs,
            edge_values,
            errors,
            removed_nodes: plan.removed_nodes.clone(),
        }
    }

    // === Internal sync executor ===========================================

    fn execute_sync_inner(
        &self,
        mode: ExecutionMode,
        on_progress: Arc<dyn Fn(ExecutionEvent) + Send + Sync>,
    ) -> Result<ExecutionResult, GraphExecutionError> {
        let plan = self.build_execution_plan()?;

        tracing::debug!(
            target: "graph",
            "[cognet] execute_sync: dirty={} removed={} mode={:?}",
            plan.dirty_nodes.len(),
            plan.removed_nodes.len(),
            mode,
        );

        if plan.is_empty() {
            return Ok(if plan.removed_nodes.is_empty() {
                ExecutionResult::empty()
            } else {
                Self::empty_result_with_removed(&plan)
            });
        }

        // Validate BEFORE clearing dirty state. RequiresAsyncExecution
        // returns the dirty set intact for a follow-up `execute_async`.
        self.validate_for_sync(&plan)?;

        self.commit_plan_drain()?;
        self.clear_execution_errors();

        let shared_nodes = self.node_manager.nodes();
        let shared_cache = self.cache.share();
        let shared_node_states = self.node_states.clone();

        let mut failed_paths: HashSet<NodePath> = HashSet::new();
        for level_nodes in &plan.levels {
            let level_vec: Vec<NodePath> = level_nodes.clone();
            let level_results: Vec<(NodePath, Result<(), String>)> = match mode {
                ExecutionMode::Sequential => level_vec
                    .into_iter()
                    .map(|path| {
                        execute_node_sync(
                            path,
                            &shared_nodes,
                            &shared_cache,
                            &shared_node_states,
                            on_progress.as_ref(),
                        )
                    })
                    .collect(),
                ExecutionMode::Parallel => level_vec
                    .into_par_iter()
                    .map(|path| {
                        execute_node_sync(
                            path,
                            &shared_nodes,
                            &shared_cache,
                            &shared_node_states,
                            on_progress.as_ref(),
                        )
                    })
                    .collect(),
            };

            for (path, res) in level_results {
                if let Err(msg) = res {
                    self.add_error(GraphError::execution_at_path(&path, msg));
                    failed_paths.insert(path);
                }
            }
        }

        // Per plan-14c §Dirty-State Rule, failures must preserve enough
        // dirty state for a later retry. Re-mark every failed node as
        // changed so the next `execute_*` call replans from the same
        // failure point. Successful nodes stay clean.
        self.restore_dirty_for_failed(failed_paths);

        Ok(self.collect_outputs(&plan))
    }

    /// Re-mark `failed_paths` as changed in `NodeStates`. Idempotent if
    /// the set is empty. Logs but does not propagate a poisoned-lock
    /// error — the executor has already finished, so the worst case is
    /// that a future caller sees a slightly stale dirty set.
    fn restore_dirty_for_failed(&self, failed_paths: HashSet<NodePath>) {
        if failed_paths.is_empty() {
            return;
        }
        match self.node_states.write() {
            Ok(mut ns) => ns.restore_changed_nodes(failed_paths),
            Err(e) => tracing::warn!(
                target: "graph",
                err = %e,
                "[cognet] could not restore dirty state for failed nodes; \
                 NodeStates lock poisoned",
            ),
        }
    }

    // === Internal async executor ==========================================

    async fn execute_async_inner(
        &self,
        mode: ExecutionMode,
        on_progress: Arc<dyn Fn(ExecutionEvent) + Send + Sync>,
    ) -> Result<ExecutionResult, GraphExecutionError> {
        let plan = self.build_execution_plan()?;

        tracing::debug!(
            target: "graph",
            "[cognet] execute_async: dirty={} removed={} mode={:?}",
            plan.dirty_nodes.len(),
            plan.removed_nodes.len(),
            mode,
        );

        if plan.is_empty() {
            return Ok(if plan.removed_nodes.is_empty() {
                ExecutionResult::empty()
            } else {
                Self::empty_result_with_removed(&plan)
            });
        }

        // Validate BEFORE draining dirty so async-subgraph rejection
        // preserves the dirty set for a later retry once async
        // subgraph support lands.
        self.validate_for_async(&plan)?;

        self.commit_plan_drain()?;
        self.clear_execution_errors();

        let shared_nodes = self.node_manager.nodes();
        let shared_cache = self.cache.share();
        let shared_node_states = self.node_states.clone();

        let mut failed_paths: HashSet<NodePath> = HashSet::new();
        for level_nodes in &plan.levels {
            // Partition dirty level into Sync/Async by execution_kind.
            let mut sync_paths: Vec<NodePath> = Vec::new();
            let mut async_paths: Vec<NodePath> = Vec::new();
            for path in level_nodes {
                match node_execution_kind(path, &shared_nodes) {
                    Some(NodeExecutionKind::AsyncIo) => async_paths.push(path.clone()),
                    // Missing entity falls through to the sync path —
                    // execute_node_sync records a per-node error.
                    _ => sync_paths.push(path.clone()),
                }
            }

            // Sync CPU work: rayon in Parallel, sequential otherwise.
            // Note: this is sync work running inside an async fn. The
            // caller's runtime can offload via `spawn_blocking` if it
            // dislikes that — the plan-14c invariant is only that the
            // graph executor itself never calls `block_on`.
            let sync_results: Vec<(NodePath, Result<(), String>)> = match mode {
                ExecutionMode::Sequential => sync_paths
                    .into_iter()
                    .map(|path| {
                        execute_node_sync(
                            path,
                            &shared_nodes,
                            &shared_cache,
                            &shared_node_states,
                            on_progress.as_ref(),
                        )
                    })
                    .collect(),
                ExecutionMode::Parallel => sync_paths
                    .into_par_iter()
                    .map(|path| {
                        execute_node_sync(
                            path,
                            &shared_nodes,
                            &shared_cache,
                            &shared_node_states,
                            on_progress.as_ref(),
                        )
                    })
                    .collect(),
            };

            for (path, res) in &sync_results {
                if let Err(msg) = res {
                    self.add_error(GraphError::execution_at_path(path, msg.clone()));
                    failed_paths.insert(path.clone());
                }
            }

            // Async I/O work: prepare each future under a short read
            // lock, drop the lock, then await all futures concurrently.
            // The lock-across-await invariant from plan-14c §Locking
            // Rule lives here.
            let mut async_futs = Vec::with_capacity(async_paths.len());
            for path in async_paths {
                on_progress(ExecutionEvent::Started(path.clone()));

                let prep_result: Result<BoxNodeFuture, String> = {
                    let entity = {
                        let guard = match shared_nodes.lock() {
                            Ok(g) => g,
                            Err(_) => {
                                self.add_error(GraphError::execution_at_path(
                                    &path,
                                    "Lock poisoned".to_string(),
                                ));
                                continue;
                            }
                        };
                        guard.get(&path).map(Arc::clone)
                    };
                    match entity {
                        None => Err("node entity missing".to_string()),
                        Some(node_entity) => {
                            let read = match node_entity.read() {
                                Ok(r) => r,
                                Err(p) => p.into_inner(),
                            };
                            // `read` (the RwLockReadGuard) is dropped at
                            // the end of this match arm — strictly BEFORE
                            // we await the prepared future, which is the
                            // plan-14c lock-across-await invariant.
                            match build_execution_context(&path, &shared_node_states, &shared_cache)
                            {
                                Ok(ctx) => read.prepare_async(ctx),
                                Err(e) => Err(e),
                            }
                        }
                    }
                };

                async_futs.push(async move {
                    match prep_result {
                        Ok(fut) => (path, fut.await),
                        Err(e) => (path, Err(e)),
                    }
                });
            }

            let async_results = futures::future::join_all(async_futs).await;

            for (path, res) in async_results {
                match res {
                    Ok(()) => on_progress(ExecutionEvent::Completed(path)),
                    Err(msg) => {
                        on_progress(ExecutionEvent::Failed(path.clone(), msg.clone()));
                        self.add_error(GraphError::execution_at_path(&path, msg));
                        failed_paths.insert(path);
                    }
                }
            }
        }

        // Per plan-14c §Dirty-State Rule: AsyncIo failures (transient
        // remote errors etc.) must leave the dirty plan re-runnable.
        self.restore_dirty_for_failed(failed_paths);

        Ok(self.collect_outputs(&plan))
    }
}

#[cfg(test)]
mod sync_executor_tests {
    use super::*;
    use crate::{AddNode, NumberNode};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc as StdArc;

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
            .get(&NodePath::root().child(node_id))
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

    // ============================================================
    // Plan-14c: hybrid sync/async execution tests
    // ============================================================

    /// Test-only `AsyncIo` node. Captures `node_data` under the short
    /// read lock that `prepare_async` holds, returns a `'static` future
    /// that yields once (proving the executor actually `.awaits` it),
    /// then forwards the value to its output. No node entity guard is
    /// held across `.await` — that is the plan-14c lock invariant.
    #[derive(Debug)]
    pub struct AsyncEchoNode;

    impl crate::NodeMeta for AsyncEchoNode {
        const NAME: &'static str = "AsyncEcho";
        const CATEGORY: crate::NodeCategory = crate::NodeCategory::Primitive;
        const INPUTS: &'static [crate::SlotDef] = &[];
        const OUTPUTS: &'static [crate::SlotDef] = &[crate::SlotDef {
            label: "Out",
            data_type: crate::DataType::Number,
            max_connections: None,
        }];
        const DEFAULT_VALUE: crate::DefaultValue = crate::DefaultValue::Number(42.0);
    }

    impl NodeImpl for AsyncEchoNode {
        fn execution_kind(&self) -> NodeExecutionKind {
            NodeExecutionKind::AsyncIo
        }

        fn prepare_async(&self, ctx: ExecutionContext) -> Result<BoxNodeFuture, String> {
            // Move the value out of the (lock-bound) ctx into the
            // 'static future. The executor drops the read lock before
            // awaiting this future.
            let data = ctx.node_data;
            let writer = ctx.output_writer;
            Ok(Box::pin(async move {
                tokio::task::yield_now().await;
                let value = data.ok_or_else(|| "AsyncEcho: missing node_data".to_string())?;
                writer.set(0, value)?;
                Ok(())
            }))
        }
    }

    crate::register_nodes!(AsyncEchoNode);

    #[test]
    fn execute_sync_returns_requires_async_for_async_node() {
        // Graph: sync NumberNode (0) -> async AsyncEchoNode (downstream).
        // The async node sits in the dirty plan, so execute_sync must
        // refuse with RequiresAsyncExecution and PRESERVE the dirty set.
        let mut graph = NodeGraph::new().unwrap();
        let echo_id = graph.create_node::<AsyncEchoNode>().unwrap();

        let err = graph.execute_sync().expect_err("sync rejects async plan");
        match err {
            GraphExecutionError::RequiresAsyncExecution { node_ids } => {
                assert!(
                    node_ids.contains(&echo_id),
                    "error must list the async node"
                );
            }
            other => panic!("expected RequiresAsyncExecution, got {other:?}"),
        }

        // Dirty preservation: the failed sync call must NOT have
        // drained `changed_nodes`. A second `execute_sync` returns the
        // same error and (more importantly) the same dirty set.
        let err2 = graph
            .execute_sync()
            .expect_err("dirty plan still rejects on second call");
        assert!(matches!(
            err2,
            GraphExecutionError::RequiresAsyncExecution { .. }
        ));
    }

    #[tokio::test]
    async fn execute_async_runs_async_node_value_propagates() {
        let mut graph = NodeGraph::new().unwrap();
        let echo_id = graph.create_node::<AsyncEchoNode>().unwrap();
        graph
            .update_node_data(&echo_id, Data::new(7.5_f64).unwrap())
            .unwrap();

        let result = graph
            .execute_async()
            .await
            .expect("async executor handles async nodes");
        assert_eq!(output_for(&result, echo_id), 7.5);
    }

    #[tokio::test]
    async fn execute_async_handles_mixed_sync_async_levels() {
        // Two sync sources feed an async sink (AsyncEcho doesn't read
        // upstream — but we sequence dirty so both kinds appear).
        let mut graph = NodeGraph::new().unwrap();
        let n1 = graph.create_node::<NumberNode>().unwrap();
        let n2 = graph.create_node::<NumberNode>().unwrap();
        let add_id = graph.create_node::<AddNode>().unwrap();
        let echo_id = graph.create_node::<AsyncEchoNode>().unwrap();

        graph
            .update_node_data(&n1, Data::new(2.0_f64).unwrap())
            .unwrap();
        graph
            .update_node_data(&n2, Data::new(5.0_f64).unwrap())
            .unwrap();
        graph
            .update_node_data(&echo_id, Data::new(11.0_f64).unwrap())
            .unwrap();
        graph.connect_nodes(&n1, 0, &add_id, 0).unwrap();
        graph.connect_nodes(&n2, 0, &add_id, 1).unwrap();

        let result = graph.execute_async().await.expect("mixed graph runs");
        assert_eq!(output_for(&result, add_id), 7.0);
        assert_eq!(output_for(&result, echo_id), 11.0);
    }

    #[tokio::test]
    async fn async_executor_drops_locks_before_awaiting_node_future() {
        // While the AsyncEcho future is suspended (yield_now), other
        // threads/tasks must be able to acquire READ locks on the graph.
        // We exercise this by reading `get_output_value` from another
        // task while the async execution is in flight.
        //
        // The test would deadlock if execute_async held a graph or node
        // lock across `.await` — that's the plan-14c lock-invariant
        // canary.
        let mut graph = NodeGraph::new().unwrap();
        let echo_id = graph.create_node::<AsyncEchoNode>().unwrap();
        graph
            .update_node_data(&echo_id, Data::new(99.0_f64).unwrap())
            .unwrap();

        let arc_graph = StdArc::new(graph);
        let g_for_reader = StdArc::clone(&arc_graph);
        let probe = tokio::spawn(async move {
            // Ten attempts to read while execution is running. Each
            // call hits the node-states / cache locks that the
            // executor would otherwise be holding.
            let mut got_some = false;
            for _ in 0..10 {
                tokio::task::yield_now().await;
                if g_for_reader.get_output_value(&echo_id, 0).is_some() {
                    got_some = true;
                }
            }
            got_some
        });

        let result = arc_graph
            .execute_async()
            .await
            .expect("async executor completes despite concurrent reads");
        assert_eq!(output_for(&result, echo_id), 99.0);
        let _ = probe.await;
    }

    #[tokio::test]
    async fn async_executor_ran_async_node_count() {
        // Also confirm that prepare_async was invoked exactly once per
        // dirty async node. We use a process-global counter because the
        // node struct itself is unit-sized.
        static PREPARE_CALLS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Debug)]
        struct CountingAsyncNode;

        impl crate::NodeMeta for CountingAsyncNode {
            const NAME: &'static str = "CountingAsync";
            const CATEGORY: crate::NodeCategory = crate::NodeCategory::Primitive;
            const INPUTS: &'static [crate::SlotDef] = &[];
            const OUTPUTS: &'static [crate::SlotDef] = &[crate::SlotDef {
                label: "Out",
                data_type: crate::DataType::Number,
                max_connections: None,
            }];
            const DEFAULT_VALUE: crate::DefaultValue = crate::DefaultValue::Number(0.0);
        }

        impl NodeImpl for CountingAsyncNode {
            fn execution_kind(&self) -> NodeExecutionKind {
                NodeExecutionKind::AsyncIo
            }
            fn prepare_async(&self, ctx: ExecutionContext) -> Result<BoxNodeFuture, String> {
                PREPARE_CALLS.fetch_add(1, Ordering::SeqCst);
                let writer = ctx.output_writer;
                let data = ctx.node_data;
                Ok(Box::pin(async move {
                    let value = data.unwrap_or(Data::new(0.0_f64).unwrap());
                    writer.set(0, value)?;
                    Ok(())
                }))
            }
        }

        crate::register_nodes!(CountingAsyncNode);

        let mut graph = NodeGraph::new().unwrap();
        let _id = graph.create_node::<CountingAsyncNode>().unwrap();
        PREPARE_CALLS.store(0, Ordering::SeqCst);
        graph.execute_async().await.expect("async run");
        assert_eq!(PREPARE_CALLS.load(Ordering::SeqCst), 1);
    }

    // ============================================================
    // Plan-14c finding 2: dirty preservation on per-node failure
    // ============================================================

    /// Test-only `AsyncIo` node that fails on a configurable counter.
    /// Allows simulating a transient remote-API error and verifying
    /// the dirty set is restored for retry.
    #[derive(Debug)]
    pub struct FlakyAsyncNode;

    /// Number of remaining failures FlakyAsyncNode should produce
    /// before succeeding. Decremented on each call.
    static FLAKY_FAIL_REMAINING: AtomicUsize = AtomicUsize::new(0);

    impl crate::NodeMeta for FlakyAsyncNode {
        const NAME: &'static str = "FlakyAsync";
        const CATEGORY: crate::NodeCategory = crate::NodeCategory::Primitive;
        const INPUTS: &'static [crate::SlotDef] = &[];
        const OUTPUTS: &'static [crate::SlotDef] = &[crate::SlotDef {
            label: "Out",
            data_type: crate::DataType::Number,
            max_connections: None,
        }];
        const DEFAULT_VALUE: crate::DefaultValue = crate::DefaultValue::Number(0.0);
    }

    impl NodeImpl for FlakyAsyncNode {
        fn execution_kind(&self) -> NodeExecutionKind {
            NodeExecutionKind::AsyncIo
        }

        fn prepare_async(&self, ctx: ExecutionContext) -> Result<BoxNodeFuture, String> {
            let writer = ctx.output_writer;
            let data = ctx.node_data;
            Ok(Box::pin(async move {
                // Simulate a transient failure pattern: fail the first
                // N attempts, succeed afterwards.
                let prev = FLAKY_FAIL_REMAINING.fetch_sub(1, Ordering::SeqCst);
                if prev > 0 {
                    return Err("transient FlakyAsync failure (test fixture)".to_string());
                }
                let value = data.unwrap_or(Data::new(0.0_f64).unwrap());
                writer.set(0, value)?;
                Ok(())
            }))
        }
    }

    crate::register_nodes!(FlakyAsyncNode);

    #[tokio::test]
    async fn async_failure_preserves_dirty_for_retry() {
        // Configure FlakyAsync to fail once. First execute_async
        // returns Ok with a per-node error in the result; the failed
        // node is re-marked dirty. A retry succeeds without manual
        // re-marking.
        FLAKY_FAIL_REMAINING.store(1, Ordering::SeqCst);

        let mut graph = NodeGraph::new().unwrap();
        let flaky_id = graph.create_node::<FlakyAsyncNode>().unwrap();
        graph
            .update_node_data(&flaky_id, Data::new(123.0_f64).unwrap())
            .unwrap();

        // First call: per-node error. Successful caller path.
        let r1 = graph
            .execute_async()
            .await
            .expect("execute_async itself does not fail on per-node errors");
        let flaky_path = NodePath::root().child(flaky_id);
        assert!(
            r1.errors.contains_key(&flaky_path),
            "first run records per-node failure"
        );
        // Output absent because the node didn't write its value.
        assert!(r1
            .node_outputs
            .get(&flaky_path)
            .and_then(|s| s.first().cloned().flatten())
            .is_none());

        // Retry without re-mark: dirty preservation guarantees the
        // failed node is in the dirty plan again.
        let r2 = graph.execute_async().await.expect("retry succeeds");
        assert!(
            !r2.errors.contains_key(&flaky_path),
            "retry produces no error"
        );
        assert_eq!(output_for(&r2, flaky_id), 123.0);
    }

    #[test]
    fn sync_failure_preserves_dirty_for_retry() {
        // Same dirty-preservation rule for sync nodes. Use a
        // counter-driven sync node so the first execute_sync errors
        // and the second succeeds.
        SYNC_FAIL_REMAINING.store(1, Ordering::SeqCst);

        let mut graph = NodeGraph::new().unwrap();
        let id = graph.create_node::<FlakySyncNode>().unwrap();
        graph
            .update_node_data(&id, Data::new(7.0_f64).unwrap())
            .unwrap();

        let r1 = graph
            .execute_sync()
            .expect("graph-level Ok despite per-node err");
        let path = NodePath::root().child(id);
        assert!(r1.errors.contains_key(&path));

        let r2 = graph.execute_sync().expect("retry succeeds");
        assert!(!r2.errors.contains_key(&path));
        assert_eq!(output_for(&r2, id), 7.0);
    }

    #[derive(Debug)]
    pub struct FlakySyncNode;

    static SYNC_FAIL_REMAINING: AtomicUsize = AtomicUsize::new(0);

    impl crate::NodeMeta for FlakySyncNode {
        const NAME: &'static str = "FlakySync";
        const CATEGORY: crate::NodeCategory = crate::NodeCategory::Primitive;
        const INPUTS: &'static [crate::SlotDef] = &[];
        const OUTPUTS: &'static [crate::SlotDef] = &[crate::SlotDef {
            label: "Out",
            data_type: crate::DataType::Number,
            max_connections: None,
        }];
        const DEFAULT_VALUE: crate::DefaultValue = crate::DefaultValue::Number(0.0);
    }

    impl NodeImpl for FlakySyncNode {
        fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
            let prev = SYNC_FAIL_REMAINING.fetch_sub(1, Ordering::SeqCst);
            if prev > 0 {
                return Err("transient FlakySync failure (test fixture)".to_string());
            }
            let data = ctx.node_data.ok_or("missing data")?;
            ctx.output_writer.set(0, data)?;
            Ok(())
        }
    }

    crate::register_nodes!(FlakySyncNode);
}
