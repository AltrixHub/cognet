pub mod convenience;
pub mod edge_info;
pub mod execution_cache;
pub mod interface_helpers;
pub mod node_graph_api;
pub mod node_graph_system;
pub mod node_manager;
pub mod node_path;
pub(crate) mod node_state;
mod path_index;
pub mod path_navigation;
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
    SubGraphNode,
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
    node_manager: NodeManager,
    /// Node states (type_name, data, slots) — shared with app via Arc.
    node_states: Arc<RwLock<NodeStates>>,
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
    ///
    /// Used by SubGraphNode to ensure all internal nodes execute
    /// after external inputs are injected into the input proxy.
    pub fn mark_all_nodes_dirty(&self) {
        if let Ok(mut ns) = self.node_states.write() {
            ns.mark_all_changed();
        }
    }

    // ── High-level query API ──
    // These methods provide access to node/slot/edge data without exposing
    // the internal NodeStates structure.

    /// Get the type name of a node.
    pub fn node_type_name(&self, node_id: &NodeId) -> Option<&'static str> {
        let ns = self.node_states.read().ok()?;
        ns.get(&NodePath::root().child(*node_id))
            .map(|s| s.type_name)
    }

    /// Get the Rust TypeId of a node for type-safe identification.
    ///
    /// For SubGraphNodes with a template type, returns the template's TypeId
    /// instead of the generic SubGraphNode TypeId. This enables callers to
    /// identify what kind of subgraph this is (e.g., StairTemplate vs WallTemplate).
    pub fn node_type_id(&self, node_id: &NodeId) -> Option<TypeId> {
        // Check for SubGraph template TypeId first
        if let Some(template_id) = self.subgraph_template_type_id(node_id) {
            return Some(template_id);
        }
        let ns = self.node_states.read().ok()?;
        let state = ns.get(&NodePath::root().child(*node_id))?;
        state.rust_type_id
    }

    /// Set the template TypeId for a SubGraphNode.
    ///
    /// This associates a template type with a SubGraphNode, allowing
    /// `node_type_id()` to return the template's TypeId instead of
    /// the generic SubGraphNode TypeId.
    pub fn set_subgraph_template_type_id(
        &self,
        node_id: &NodeId,
        type_id: TypeId,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
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
        let entity = self.get_node_by_id(node_id)?;
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
        self.node_states
            .read()
            .ok()
            .map(|ns| ns.input_slot_count(&NodePath::root().child(*node_id)))
            .unwrap_or(0)
    }

    /// Get output slot count for a node.
    pub fn output_slot_count(&self, node_id: &NodeId) -> usize {
        self.node_states
            .read()
            .ok()
            .map(|ns| ns.output_slot_count(&NodePath::root().child(*node_id)))
            .unwrap_or(0)
    }

    /// Get the label of an input slot.
    pub fn input_slot_label(&self, node_id: &NodeId, slot: usize) -> Option<&'static str> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| s.label)
    }

    /// Get the data type of an input slot.
    pub fn input_slot_data_type(&self, node_id: &NodeId, slot: usize) -> Option<DataType> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| s.data_type)
    }

    /// Get the default value of an input slot.
    pub fn input_slot_default_value(&self, node_id: &NodeId, slot: usize) -> Option<DataValue> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(&NodePath::root().child(*node_id), slot)
            .and_then(|s| s.default_value.as_ref().map(Arc::clone))
    }

    /// Get combined info for an input slot (label + data_type + default_value).
    pub fn input_slot_info(&self, node_id: &NodeId, slot: usize) -> Option<InputSlotInfo> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| InputSlotInfo {
                id: s.id,
                label: s.label,
                data_type: s.data_type,
                default_value: s.default_value.as_ref().map(Arc::clone),
            })
    }

    /// Get combined info for an output slot (label + data_type).
    pub fn output_slot_info(&self, node_id: &NodeId, slot: usize) -> Option<SlotInfo> {
        let ns = self.node_states.read().ok()?;
        ns.output_slot(&NodePath::root().child(*node_id), slot)
            .map(|s| SlotInfo {
                label: s.label,
                data_type: s.data_type,
            })
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

    /// Get an edge by ID.
    pub fn get_edge_by_id(&self, edge_id: &EdgeId) -> Option<Edge> {
        let ns = self.node_states.read().ok()?;
        ns.get_edge(edge_id).cloned()
    }

    /// Get edges connected to an input slot by (node_id, slot_index).
    pub fn edges_for_input_slot(&self, node_id: &NodeId, slot: usize) -> Vec<EdgeId> {
        self.node_states
            .read()
            .ok()
            .and_then(|ns| {
                let slot_state = ns.input_slot(&NodePath::root().child(*node_id), slot)?;
                Some(ns.edges_for_input(&slot_state.id).to_vec())
            })
            .unwrap_or_default()
    }

    /// Get edges connected from a specific output slot (ordered).
    pub fn edges_for_output_slot(&self, node_id: &NodeId, slot: usize) -> Vec<EdgeId> {
        self.node_states
            .read()
            .ok()
            .and_then(|ns| {
                let slot_state = ns.output_slot(&NodePath::root().child(*node_id), slot)?;
                Some(ns.edges_for_output(&slot_state.id).to_vec())
            })
            .unwrap_or_default()
    }

    /// Reorder an edge within its output slot's connection list.
    pub fn reorder_output_edge(&self, edge_id: &EdgeId, new_index: usize) -> bool {
        self.node_states
            .write()
            .ok()
            .is_some_and(|mut ns| ns.reorder_output_edge(edge_id, new_index))
    }

    /// Reorder an edge within its input slot's connection list.
    ///
    /// Changes the position of the edge to `new_index`, affecting the order
    /// in which multi-input values are received during execution.
    /// Returns `true` if the reorder was successful.
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
    #[test]
    fn test_execute() {}
}
