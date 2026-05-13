//! Runtime state for node instances.

use crate::{
    Data, DataType, DataValue, Edge, EdgeId, InputSlotId, NodeId, NodePath, OutputSlotId, SlotDef,
};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use super::path_index::PathIndex;

/// Runtime state for a single node instance.
#[derive(Debug, Clone)]
pub(crate) struct NodeState {
    /// The node type name (references NodeMeta::NAME).
    pub type_name: &'static str,
    /// Rust TypeId for type-safe identification (None for legacy/untyped nodes).
    pub rust_type_id: Option<TypeId>,
    /// Current data value (can be modified at runtime).
    pub data: Option<Data>,
}

impl NodeState {
    pub fn new(
        type_name: &'static str,
        type_id: Option<TypeId>,
        default_data: Option<Data>,
    ) -> Self {
        Self {
            type_name,
            rust_type_id: type_id,
            data: default_data,
        }
    }
}

/// Runtime state for input slots.
#[derive(Debug, Default, Clone)]
pub(crate) struct InputSlotState {
    pub id: InputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub max_connections: Option<usize>,
    pub default_value: Option<DataValue>,
}

/// Runtime state for output slots.
#[derive(Debug, Default, Clone)]
pub(crate) struct OutputSlotState {
    pub id: OutputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
}

/// Shared handle to `NodeStates` for cross-thread access.
pub(crate) type SharedNodeStates = Arc<RwLock<NodeStates>>;

/// Manages all node states in the graph.
#[derive(Debug, Default, Clone)]
pub(crate) struct NodeStates {
    /// Node data by NodePath.
    nodes: HashMap<NodePath, NodeState>,
    /// Input slot states by (NodePath, slot_index).
    input_slots: HashMap<(NodePath, usize), InputSlotState>,
    /// Output slot states by (NodePath, slot_index).
    output_slots: HashMap<(NodePath, usize), OutputSlotState>,
    /// All edges in the graph.
    edges: HashMap<EdgeId, Edge>,
    /// Index: input slot → connected edge IDs (ordered).
    input_connections: HashMap<InputSlotId, Vec<EdgeId>>,
    /// Index: output slot → connected edge IDs (ordered).
    output_connections: HashMap<OutputSlotId, Vec<EdgeId>>,
    /// Index: source node → outgoing edge IDs (NodePath-keyed).
    outgoing_edges: HashMap<NodePath, Vec<EdgeId>>,
    /// Nodes changed since last drain (NodePath-keyed for deferred dirty tracking).
    changed_nodes: HashSet<NodePath>,
    /// Hierarchical index: parent NodePath → direct children NodeIds.
    path_index: PathIndex,
}

impl NodeStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new node with slot metadata from `SlotDef` arrays.
    pub fn add_node(
        &mut self,
        path: &NodePath,
        type_name: &'static str,
        type_id: Option<TypeId>,
        default_data: Option<Data>,
        input_defs: &[SlotDef],
        output_defs: &[SlotDef],
    ) {
        // Register in path_index so subtree walks can find this node.
        self.path_index.insert(path);

        self.nodes.insert(
            path.clone(),
            NodeState::new(type_name, type_id, default_data),
        );

        // Initialize input slot states with metadata
        for (i, def) in input_defs.iter().enumerate() {
            self.input_slots.insert(
                (path.clone(), i),
                InputSlotState {
                    id: InputSlotId::new(),
                    label: def.label,
                    data_type: def.data_type,
                    max_connections: def.max_connections,
                    default_value: None,
                },
            );
        }

        // Initialize output slot states with metadata
        for (i, def) in output_defs.iter().enumerate() {
            self.output_slots.insert(
                (path.clone(), i),
                OutputSlotState {
                    id: OutputSlotId::new(),
                    label: def.label,
                    data_type: def.data_type,
                },
            );
        }

        // Mark the node as changed.
        self.changed_nodes.insert(path.clone());
    }

    /// Add a dynamic input slot (for SubGraphNode).
    pub fn add_input_slot(
        &mut self,
        path: &NodePath,
        label: &'static str,
        data_type: DataType,
        max_connections: Option<usize>,
    ) -> usize {
        let index = self.input_slot_count(path);
        self.input_slots.insert(
            (path.clone(), index),
            InputSlotState {
                id: InputSlotId::new(),
                label,
                data_type,
                max_connections,
                default_value: None,
            },
        );
        index
    }

    /// Add a dynamic output slot (for SubGraphNode).
    pub fn add_output_slot(
        &mut self,
        path: &NodePath,
        label: &'static str,
        data_type: DataType,
    ) -> usize {
        let index = self.output_slot_count(path);
        self.output_slots.insert(
            (path.clone(), index),
            OutputSlotState {
                id: OutputSlotId::new(),
                label,
                data_type,
            },
        );
        index
    }

    /// Insert an output slot at a specific index, shifting higher-indexed slots up.
    ///
    /// Used by dynamic multi-input to insert proxy outputs at the correct
    /// position so that existing edge references to higher slots remain valid.
    pub fn insert_output_slot_at(
        &mut self,
        path: &NodePath,
        index: usize,
        label: &'static str,
        data_type: DataType,
    ) -> OutputSlotId {
        let count = self.output_slot_count(path);
        // Shift existing slots at index..count up by 1 (iterate in reverse)
        for i in (index..count).rev() {
            if let Some(slot) = self.output_slots.remove(&(path.clone(), i)) {
                self.output_slots.insert((path.clone(), i + 1), slot);
            }
        }
        // Insert new slot at index
        let id = OutputSlotId::new();
        self.output_slots.insert(
            (path.clone(), index),
            OutputSlotState {
                id,
                label,
                data_type,
            },
        );
        id
    }

    /// Rewrite the label of an input slot in place. Slot index and id
    /// are preserved so existing edges remain valid.
    pub fn set_input_slot_label(
        &mut self,
        path: &NodePath,
        index: usize,
        label: &'static str,
    ) -> bool {
        if let Some(slot) = self.input_slots.get_mut(&(path.clone(), index)) {
            slot.label = label;
            true
        } else {
            false
        }
    }

    /// Rewrite the label of an output slot in place. Slot index and id
    /// are preserved so existing edges remain valid.
    pub fn set_output_slot_label(
        &mut self,
        path: &NodePath,
        index: usize,
        label: &'static str,
    ) -> bool {
        if let Some(slot) = self.output_slots.get_mut(&(path.clone(), index)) {
            slot.label = label;
            true
        } else {
            false
        }
    }

    /// Remove a dynamic input slot by index and shift higher-indexed slots down.
    ///
    /// Edges referencing this node's input slot have their
    /// `to_input_slot_index` decremented when greater than `index`.
    /// Callers must drop edges that touch the removed slot before
    /// calling this — surviving edges keep their `to_input_slot_id`,
    /// only the index is adjusted.
    pub fn remove_input_slot(&mut self, path: &NodePath, index: usize) {
        let count = self.input_slot_count(path);
        if index >= count {
            return;
        }
        // Remove the slot at index
        self.input_slots.remove(&(path.clone(), index));
        // Shift slots above index down by 1
        for i in (index + 1)..count {
            if let Some(slot) = self.input_slots.remove(&(path.clone(), i)) {
                self.input_slots.insert((path.clone(), i - 1), slot);
            }
        }
        // Decrement to_input_slot_index on edges that reference this
        // node's higher-indexed input slots so indices stay aligned.
        let node_id = path.leaf();
        for edge in self.edges.values_mut() {
            if Some(edge.to_node_id) == node_id && edge.to_input_slot_index > index {
                edge.to_input_slot_index -= 1;
            }
        }
    }

    /// Remove a dynamic output slot by index and shift higher-indexed slots down.
    ///
    /// Edges referencing this node's output slot have their
    /// `from_output_slot_index` decremented when greater than `index`.
    /// Callers must drop edges that touch the removed slot before
    /// calling this — surviving edges keep their `from_output_slot_id`,
    /// only the index is adjusted.
    pub fn remove_output_slot(&mut self, path: &NodePath, index: usize) {
        let count = self.output_slot_count(path);
        if index >= count {
            return;
        }
        // Remove the slot at index
        self.output_slots.remove(&(path.clone(), index));
        // Shift slots above index down by 1
        for i in (index + 1)..count {
            if let Some(slot) = self.output_slots.remove(&(path.clone(), i)) {
                self.output_slots.insert((path.clone(), i - 1), slot);
            }
        }
        // Decrement from_output_slot_index on edges that reference this
        // node's higher-indexed output slots so indices stay aligned.
        let node_id = path.leaf();
        for edge in self.edges.values_mut() {
            if Some(edge.from_node_id) == node_id && edge.from_output_slot_index > index {
                edge.from_output_slot_index -= 1;
            }
        }
    }

    /// Count input slots for a node.
    pub fn input_slot_count(&self, path: &NodePath) -> usize {
        self.input_slots.keys().filter(|(p, _)| p == path).count()
    }

    /// Count output slots for a node.
    pub fn output_slot_count(&self, path: &NodePath) -> usize {
        self.output_slots.keys().filter(|(p, _)| p == path).count()
    }

    /// Remove a node, its slot states, and all connected edges.
    pub fn remove_node(&mut self, path: &NodePath) -> Option<NodeState> {
        // Tear down the path_index entry.
        self.path_index.remove(path);

        // Remove all edges involving this node (maintains indexes).
        // `path.leaf()` cannot be None here: remove_node is never called
        // on the root path. A non-leaf path would silently skip edge
        // teardown, so make the contract explicit.
        let node_id = path
            .leaf()
            .expect("remove_node requires a non-root NodePath");
        self.remove_edges_for_node(&node_id);

        // Remove slot states
        self.input_slots.retain(|(p, _), _| p != path);
        self.output_slots.retain(|(p, _), _| p != path);

        self.nodes.remove(path)
    }

    /// Get node state.
    pub fn get(&self, path: &NodePath) -> Option<&NodeState> {
        self.nodes.get(path)
    }

    /// Get mutable node state.
    pub fn get_mut(&mut self, path: &NodePath) -> Option<&mut NodeState> {
        self.nodes.get_mut(path)
    }

    /// Get input slot state.
    pub fn input_slot(&self, path: &NodePath, slot_index: usize) -> Option<&InputSlotState> {
        self.input_slots.get(&(path.clone(), slot_index))
    }

    /// Get mutable input slot state.
    pub fn input_slot_mut(
        &mut self,
        path: &NodePath,
        slot_index: usize,
    ) -> Option<&mut InputSlotState> {
        self.input_slots.get_mut(&(path.clone(), slot_index))
    }

    /// Get output slot state.
    pub fn output_slot(&self, path: &NodePath, slot_index: usize) -> Option<&OutputSlotState> {
        self.output_slots.get(&(path.clone(), slot_index))
    }

    /// Get all leaf NodeIds (compatibility helper for callers that only need flat IDs).
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().filter_map(|p| p.leaf())
    }

    /// Add an edge and update lookup indexes.
    ///
    /// P3b note: `Edge.from_node_id` / `to_node_id` are still bare `NodeId`,
    /// so we root-wrap here. P3c migrates `Edge` to carry `NodePath` and
    /// removes the `NodePath::root().child(_)` calls in this method.
    pub fn add_edge(&mut self, edge_id: EdgeId, edge: Edge) {
        // P3b: Edge.to_node_id is NodeId; root-wrap until P3c.
        self.changed_nodes
            .insert(NodePath::root().child(edge.to_node_id));
        self.input_connections
            .entry(edge.to_input_slot_id)
            .or_default()
            .push(edge_id);
        self.output_connections
            .entry(edge.from_output_slot_id)
            .or_default()
            .push(edge_id);
        // P3b: Edge.from_node_id is NodeId; root-wrap until P3c.
        self.outgoing_edges
            .entry(NodePath::root().child(edge.from_node_id))
            .or_default()
            .push(edge_id);
        self.edges.insert(edge_id, edge);
    }

    /// Remove an edge and update lookup indexes.
    ///
    /// P3b note: Edge fields are still bare NodeIds; root-wrap removed in P3c.
    pub fn remove_edge(&mut self, edge_id: &EdgeId) -> Option<Edge> {
        if let Some(edge) = self.edges.remove(edge_id) {
            // P3b: Edge.to_node_id is NodeId; root-wrap until P3c.
            self.changed_nodes
                .insert(NodePath::root().child(edge.to_node_id));
            if let Some(connections) = self.input_connections.get_mut(&edge.to_input_slot_id) {
                connections.retain(|id| id != edge_id);
            }
            if let Some(connections) = self.output_connections.get_mut(&edge.from_output_slot_id) {
                connections.retain(|id| id != edge_id);
            }
            // P3b: Edge.from_node_id is NodeId; root-wrap until P3c.
            let from_path = NodePath::root().child(edge.from_node_id);
            if let Some(outgoing) = self.outgoing_edges.get_mut(&from_path) {
                outgoing.retain(|id| id != edge_id);
            }
            Some(edge)
        } else {
            None
        }
    }

    /// Remove all edges involving a node, maintaining all indexes.
    pub fn remove_edges_for_node(&mut self, node_id: &NodeId) {
        let edge_ids: Vec<EdgeId> = self
            .edges
            .iter()
            .filter(|(_, edge)| edge.from_node_id == *node_id || edge.to_node_id == *node_id)
            .map(|(id, _)| *id)
            .collect();
        for edge_id in edge_ids {
            self.remove_edge(&edge_id);
        }
    }

    /// Get an edge by ID.
    pub fn get_edge(&self, edge_id: &EdgeId) -> Option<&Edge> {
        self.edges.get(edge_id)
    }

    /// Get all edges.
    pub fn edges(&self) -> &HashMap<EdgeId, Edge> {
        &self.edges
    }

    /// Get edges connected to an input slot (ordered).
    pub fn edges_for_input(&self, input_slot_id: &InputSlotId) -> &[EdgeId] {
        self.input_connections
            .get(input_slot_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get edges connected from an output slot (ordered).
    pub fn edges_for_output(&self, output_slot_id: &OutputSlotId) -> &[EdgeId] {
        self.output_connections
            .get(output_slot_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Reorder an edge within its output slot's connection list.
    pub fn reorder_output_edge(&mut self, edge_id: &EdgeId, new_index: usize) -> bool {
        let Some(edge) = self.edges.get(edge_id) else {
            return false;
        };
        let slot_id = edge.from_output_slot_id;
        let from_node_id = edge.from_node_id;

        let Some(connections) = self.output_connections.get_mut(&slot_id) else {
            return false;
        };
        let Some(old_index) = connections.iter().position(|id| id == edge_id) else {
            return false;
        };

        let id = connections.remove(old_index);
        let insert_at = new_index.min(connections.len());
        connections.insert(insert_at, id);

        // P3b: Edge.from_node_id is NodeId; root-wrap until P3c.
        self.changed_nodes
            .insert(NodePath::root().child(from_node_id));
        true
    }

    /// Reorder an edge within its input slot's connection list.
    ///
    /// Moves the edge from its current position to `new_index`.
    /// Returns `true` if the reorder was successful.
    pub fn reorder_input_edge(&mut self, edge_id: &EdgeId, new_index: usize) -> bool {
        let Some(edge) = self.edges.get(edge_id) else {
            return false;
        };
        let slot_id = edge.to_input_slot_id;
        let to_node_id = edge.to_node_id;

        let Some(connections) = self.input_connections.get_mut(&slot_id) else {
            return false;
        };
        let Some(old_index) = connections.iter().position(|id| id == edge_id) else {
            return false;
        };

        let id = connections.remove(old_index);
        let insert_at = new_index.min(connections.len());
        connections.insert(insert_at, id);

        // Mark the target node as changed so the graph re-executes.
        // P3b: Edge.to_node_id is NodeId; root-wrap until P3c.
        self.changed_nodes
            .insert(NodePath::root().child(to_node_id));
        true
    }

    /// Get outgoing edge IDs from a node.
    pub fn outgoing_edges_at(&self, path: &NodePath) -> &[EdgeId] {
        self.outgoing_edges
            .get(path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Mark a node as changed (needs re-execution).
    pub fn mark_changed(&mut self, path: &NodePath) {
        self.changed_nodes.insert(path.clone());
    }

    /// Mark all nodes under (and including) `root` as changed.
    ///
    /// Uses the `path_index` to walk the subtree so that SubGraph
    /// descendants are also marked dirty when a parent is re-executed.
    pub fn mark_all_changed_at(&mut self, root: &NodePath) {
        let mut stack: Vec<NodePath> = vec![root.clone()];
        while let Some(p) = stack.pop() {
            if self.nodes.contains_key(&p) {
                self.changed_nodes.insert(p.clone());
            }
            for child in self.path_index.children_of(&p) {
                stack.push(p.child(*child));
            }
        }
    }

    /// Mark all nodes as changed (forces full re-execution).
    ///
    /// Convenience wrapper that calls `mark_all_changed_at` from root,
    /// covering every node registered in this `NodeStates`.
    pub fn mark_all_changed(&mut self) {
        self.mark_all_changed_at(&NodePath::root());
    }

    /// Drain and return all changed node paths since last drain.
    pub fn drain_changed_nodes(&mut self) -> HashSet<NodePath> {
        std::mem::take(&mut self.changed_nodes)
    }

    /// Snapshot the current changed-node set without clearing it.
    ///
    /// The plan-14c executor calls this during planning to validate the
    /// dirty plan against the selected execution API. If validation
    /// fails (e.g. sync execute on an async-required graph), the dirty
    /// set must remain intact so a follow-up async run can re-plan.
    pub fn peek_changed_nodes(&self) -> HashSet<NodePath> {
        self.changed_nodes.clone()
    }

    /// Re-mark a set of nodes as changed.
    ///
    /// The plan-14c executor calls this after execution to restore
    /// dirty state for nodes that failed (e.g. transient AsyncIo
    /// errors), so a follow-up `execute_async` retry can re-run those
    /// nodes against the same plan without losing the change record.
    pub fn restore_changed_nodes(&mut self, paths: HashSet<NodePath>) {
        self.changed_nodes.extend(paths);
    }
}
