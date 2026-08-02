pub mod convenience;
pub mod edge_info;
pub mod execution_cache;
pub mod interface_helpers;
pub mod node_graph_api;
pub mod node_graph_system;
pub mod node_manager;
pub mod node_path;
pub(crate) mod node_state;
pub mod path_api;
mod path_index;
pub mod subgraph_helpers;
pub mod subgraph_ops;

pub use edge_info::*;
pub use execution_cache::*;
pub use node_graph_api::*;
pub use node_manager::*;
pub use node_path::NodePath;
pub(crate) use node_state::*;
pub use subgraph_ops::*;

use crate::{
    Data, DataType, DataValue, Edge, EdgeId, ErrorTarget, GraphError, InputSlotId, NodeId,
    OutputSlotId, SubGraphNode,
};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Combined slot information returned by high-level query methods.
#[derive(Debug, Clone)]
pub struct SlotInfo {
    pub label: &'static str,
    pub data_type: DataType,
}

/// Combined input slot information including default value.
#[derive(Debug, Clone)]
pub struct InputSlotInfo {
    pub id: InputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub default_value: Option<DataValue>,
}

/// Mutable bookkeeping state used during execution and graph mutation.
///
/// Wrapped in `Mutex` for interior mutability so that `execute()` can
/// take `&self` instead of `&mut self`, eliminating the need for the
/// app layer to hold a write lock on `NodeGraph` during execution.
#[derive(Default)]
struct Bookkeeping {
    errors: HashMap<ErrorTarget, GraphError>,
    removed_since_last_execute: Vec<NodeId>,
}

pub struct NodeGraph {
    pub(crate) node_manager: NodeManager,
    /// Node states (type_name, data, slots) — shared with app via Arc.
    pub(crate) node_states: Arc<RwLock<NodeStates>>,
    cache: SharedExecutionCache,
    /// Interior-mutable bookkeeping (dirty_nodes, errors, removed list).
    bookkeeping: Mutex<Bookkeeping>,
}

impl NodeGraph {
    /// Create a new NodeGraph.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            node_manager: NodeManager::new()?,
            node_states: Arc::new(RwLock::new(NodeStates::new())),
            cache: Default::default(),
            bookkeeping: Mutex::new(Bookkeeping::default()),
        })
    }

    /// Get shared reference to the NodeStates storage.
    pub(crate) fn node_states(&self) -> &Arc<RwLock<NodeStates>> {
        &self.node_states
    }

    /// Get shared execution cache (for UI access to output values).
    pub fn shared_cache(&self) -> SharedExecutionCache {
        self.cache.share()
    }

    /// Get all current errors (snapshot).
    pub fn errors(&self) -> HashMap<ErrorTarget, GraphError> {
        self.bookkeeping
            .lock()
            .map(|b| b.errors.clone())
            .unwrap_or_default()
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        self.bookkeeping
            .lock()
            .map(|b| !b.errors.is_empty())
            .unwrap_or(false)
    }

    /// Get error for a specific target.
    pub fn error_for(&self, target: &ErrorTarget) -> Option<GraphError> {
        self.bookkeeping
            .lock()
            .ok()
            .and_then(|b| b.errors.get(target).cloned())
    }

    /// Add an error.
    pub fn add_error(&self, error: GraphError) {
        if let Ok(mut b) = self.bookkeeping.lock() {
            b.errors.insert(error.target.clone(), error);
        }
    }

    /// Clear error for a specific target.
    pub fn clear_error(&self, target: &ErrorTarget) {
        if let Ok(mut b) = self.bookkeeping.lock() {
            b.errors.remove(target);
        }
    }

    /// Clear all errors.
    pub fn clear_all_errors(&self) {
        if let Ok(mut b) = self.bookkeeping.lock() {
            b.errors.clear();
        }
    }

    /// Clear all execution errors (before running execute()).
    pub fn clear_execution_errors(&self) {
        if let Ok(mut b) = self.bookkeeping.lock() {
            b.errors.retain(|_, e| !e.is_execution_error());
        }
    }

    /// Mark all nodes in the graph as dirty, forcing re-execution.
    pub fn mark_all_nodes_dirty(&self) {
        if let Ok(mut ns) = self.node_states.write() {
            ns.mark_all_changed();
        }
    }

    // ── High-level query API ──

    /// Get the type name of a node.
    pub fn node_type_name(&self, node_id: &NodeId) -> Option<&'static str> {
        self.node_type_name_at(&NodePath::root().child(*node_id))
    }

    /// Get the Rust TypeId of a node for type-safe identification.
    ///
    /// For SubGraphNodes with a template type, returns the template's TypeId
    /// instead of the generic SubGraphNode TypeId.
    pub fn node_type_id(&self, node_id: &NodeId) -> Option<TypeId> {
        if let Some(template_id) = self.subgraph_template_type_id(node_id) {
            return Some(template_id);
        }
        let ns = self.node_states.read().ok()?;
        let state = ns.get(&NodePath::root().child(*node_id))?;
        state.rust_type_id
    }

    /// Set the template TypeId for a SubGraphNode.
    pub fn set_subgraph_template_type_id(
        &self,
        node_id: &NodeId,
        type_id: TypeId,
    ) -> Result<(), String> {
        let entity = self
            .node_manager
            .get_at(&NodePath::root().child(*node_id))
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut write = entity.write().map_err(|e| e.to_string())?;
        let sg = write
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.set_template_type_id(type_id);
        Ok(())
    }

    /// Get the template TypeId of a SubGraphNode.
    pub fn subgraph_template_type_id(&self, node_id: &NodeId) -> Option<TypeId> {
        let entity = self
            .node_manager
            .get_at(&NodePath::root().child(*node_id))?;
        let read = entity.read().ok()?;
        let sg = read.as_any().downcast_ref::<SubGraphNode>()?;
        sg.template_type_id()
    }

    /// Get the data value of a node.
    pub fn node_data(&self, node_id: &NodeId) -> Option<Data> {
        let ns = self.node_states.read().ok()?;
        ns.get(&NodePath::root().child(*node_id))
            .and_then(|s| s.data.as_ref().map(|d| d.share()))
    }

    /// Get the data value of a node at the given path. Path-aware sibling of
    /// [`node_data`](Self::node_data); needed by consumers (e.g. catalog
    /// populate, interface-bidirectional proxy) that reference depth-2 nodes
    /// living inside SubGraph templates.
    pub fn node_data_at_path(&self, path: &NodePath) -> Option<Data> {
        let ns = self.node_states.read().ok()?;
        ns.get(path)
            .and_then(|state| state.data.as_ref().map(|d| d.share()))
    }

    /// Check if a node exists.
    pub fn has_node(&self, node_id: &NodeId) -> bool {
        self.node_states
            .read()
            .ok()
            .map(|ns| ns.get(&NodePath::root().child(*node_id)).is_some())
            .unwrap_or(false)
    }

    /// Get input slot count for a node.
    pub fn input_slot_count(&self, node_id: &NodeId) -> usize {
        self.input_slot_count_at(&NodePath::root().child(*node_id))
    }

    /// Get output slot count for a node.
    pub fn output_slot_count(&self, node_id: &NodeId) -> usize {
        self.output_slot_count_at(&NodePath::root().child(*node_id))
    }

    /// Get the label of an input slot.
    pub fn input_slot_label(&self, node_id: &NodeId, slot: usize) -> Option<&'static str> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| s.label)
    }

    /// Get the data type of an input slot.
    pub fn input_slot_data_type(&self, node_id: &NodeId, slot: usize) -> Option<DataType> {
        self.input_slot_data_type_at(&NodePath::root().child(*node_id), slot)
    }

    /// Whether an input slot should be rendered as an editable row by
    /// downstream property inspectors. Path-aware sibling lives at
    /// [`NodeGraph::input_slot_inspector_visible_at`].
    pub fn input_slot_inspector_visible(&self, node_id: &NodeId, slot: usize) -> Option<bool> {
        self.input_slot_inspector_visible_at(&NodePath::root().child(*node_id), slot)
    }

    /// Get the default value of an input slot.
    pub fn input_slot_default_value(&self, node_id: &NodeId, slot: usize) -> Option<DataValue> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(&NodePath::root().child(*node_id), slot)
            .and_then(|s| s.default_value.as_ref().map(Arc::clone))
    }

    /// Get combined info for an input slot (label + data_type + default_value).
    pub fn input_slot_info(&self, node_id: &NodeId, slot: usize) -> Option<InputSlotInfo> {
        self.input_slot_info_at(&NodePath::root().child(*node_id), slot)
    }

    /// Get combined info for an output slot (label + data_type).
    pub fn output_slot_info(&self, node_id: &NodeId, slot: usize) -> Option<SlotInfo> {
        self.output_slot_info_at(&NodePath::root().child(*node_id), slot)
    }

    /// Get the InputSlotId for an input slot.
    pub fn input_slot_id(&self, node_id: &NodeId, slot: usize) -> Option<InputSlotId> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| s.id)
    }

    /// Get the label of an output slot.
    pub fn output_slot_label(&self, node_id: &NodeId, slot: usize) -> Option<&'static str> {
        let ns = self.node_states.read().ok()?;
        ns.output_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| s.label)
    }

    /// Get the data type of an output slot.
    pub fn output_slot_data_type(&self, node_id: &NodeId, slot: usize) -> Option<DataType> {
        let ns = self.node_states.read().ok()?;
        ns.output_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| s.data_type)
    }

    /// Get all edges (cloned).
    pub fn edges(&self) -> HashMap<EdgeId, Edge> {
        self.node_states
            .read()
            .ok()
            .map(|ns| ns.edges().clone())
            .unwrap_or_default()
    }

    /// The graph's structural generation — a monotone counter that moves
    /// whenever the graph's SHAPE changes.
    ///
    /// "Shape" is everything a topology query can observe: which nodes
    /// exist, which slots they carry (count, order, label, data type,
    /// default value) and which edges connect them. Node *data* writes
    /// (`update_node_data*`) and execution do NOT move it — they change
    /// what a node computes, never how the graph is wired.
    ///
    /// This is the identity a consumer caches a graph-shaped projection
    /// against: read it, build the projection, and rebuild only once the
    /// value has moved. The counter is conservative — a mutation that
    /// happens to leave the shape identical still bumps it — so a stale
    /// cache is impossible and a redundant rebuild is merely wasted work.
    ///
    /// Returns `0` on a poisoned state lock, which reads as "the shape
    /// may have changed" for any cache that stored a non-zero value.
    pub fn structure_generation(&self) -> u64 {
        self.node_states
            .read()
            .map(|ns| ns.structure_generation())
            .unwrap_or(0)
    }

    /// Get an edge by ID.
    pub fn get_edge_by_id(&self, edge_id: &EdgeId) -> Option<Edge> {
        let ns = self.node_states.read().ok()?;
        ns.get_edge(edge_id).cloned()
    }

    /// Get edges connected to an input slot by (node_id, slot_index)
    /// (ordered). Thin wrapper over
    /// [`edges_for_input_slot_at`](Self::edges_for_input_slot_at).
    pub fn edges_for_input_slot(&self, node_id: &NodeId, slot: usize) -> Vec<EdgeId> {
        self.edges_for_input_slot_at(&NodePath::root().child(*node_id), slot)
    }

    /// Get edges connected from a specific output slot (ordered). Thin
    /// wrapper over
    /// [`edges_for_output_slot_at`](Self::edges_for_output_slot_at).
    pub fn edges_for_output_slot(&self, node_id: &NodeId, slot: usize) -> Vec<EdgeId> {
        self.edges_for_output_slot_at(&NodePath::root().child(*node_id), slot)
    }

    /// Look up the owning `(NodePath, slot_index)` for an `InputSlotId`.
    ///
    /// Returns `None` if the slot has been removed or was never registered.
    /// Thin forwarding wrapper around `NodeStates::input_slot_owner`
    /// (plan-006 C17).
    pub fn input_slot_owner(&self, sid: &InputSlotId) -> Option<(NodePath, usize)> {
        self.node_states.read().ok()?.input_slot_owner(sid).cloned()
    }

    /// Look up the owning `(NodePath, slot_index)` for an `OutputSlotId`.
    ///
    /// Thin forwarding wrapper around `NodeStates::output_slot_owner`
    /// (plan-006 C17).
    pub fn output_slot_owner(&self, sid: &OutputSlotId) -> Option<(NodePath, usize)> {
        self.node_states
            .read()
            .ok()?
            .output_slot_owner(sid)
            .cloned()
    }

    /// Reorder an edge within its output slot's connection list.
    pub fn reorder_output_edge(&self, edge_id: &EdgeId, new_index: usize) -> bool {
        self.node_states
            .write()
            .ok()
            .is_some_and(|mut ns| ns.reorder_output_edge(edge_id, new_index))
    }

    /// Reorder an edge within its input slot's connection list.
    pub fn reorder_input_edge(&self, edge_id: &EdgeId, new_index: usize) -> bool {
        self.node_states
            .write()
            .ok()
            .is_some_and(|mut ns| ns.reorder_input_edge(edge_id, new_index))
    }

    /// Get outgoing edge IDs from a node.
    pub fn outgoing_edges(&self, node_id: &NodeId) -> Vec<EdgeId> {
        self.node_states
            .read()
            .ok()
            .map(|ns| {
                ns.outgoing_edges_at(&NodePath::root().child(*node_id))
                    .to_vec()
            })
            .unwrap_or_default()
    }

    /// Record a node removal (will be reported in next execute()).
    pub(crate) fn record_removal(&self, node_id: NodeId) {
        if let Ok(mut b) = self.bookkeeping.lock() {
            b.removed_since_last_execute.push(node_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Data, NodeGraph, NodePath};

    #[test]
    fn test_execute() {}

    /// The contract [`NodeGraph::structure_generation`] promises: it
    /// moves on wiring changes and stands still on data writes and
    /// execution. A projection cache keyed on it is only correct if BOTH
    /// halves hold.
    #[test]
    fn structure_generation_moves_only_on_shape_changes() {
        let mut graph = NodeGraph::new().expect("create graph");
        let start = graph.structure_generation();

        let sg = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add subgraph");
        let after_node = graph.structure_generation();
        assert!(after_node > start, "creating a node must move the counter");

        let sg_path = NodePath::root().child(sg);
        let a = graph
            .create_node_by_name_at(&sg_path, "Number")
            .expect("create Number a");
        let b = graph
            .create_node_by_name_at(&sg_path, "Add")
            .expect("create Add b");
        let after_children = graph.structure_generation();
        assert!(after_children > after_node);

        graph
            .connect_nodes_at(&sg_path.child(a), 0, &sg_path.child(b), 0)
            .expect("connect a → b");
        let after_edge = graph.structure_generation();
        assert!(after_edge > after_children, "an edge must move the counter");

        // Data writes and execution leave the shape alone.
        graph
            .update_node_data_at(&sg_path.child(a), Data::new(7.0_f64).expect("number"))
            .expect("write node data");
        assert_eq!(
            graph.structure_generation(),
            after_edge,
            "a node-data write must NOT move the counter",
        );
        graph.execute_sync().expect("execute");
        assert_eq!(
            graph.structure_generation(),
            after_edge,
            "execution must NOT move the counter",
        );

        graph.remove_node_at(&sg_path.child(b)).expect("remove b");
        assert!(
            graph.structure_generation() > after_edge,
            "removing a node must move the counter",
        );
    }
}
