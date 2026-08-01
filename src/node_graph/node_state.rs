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
#[derive(Debug, Clone)]
pub(crate) struct InputSlotState {
    pub id: InputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub max_connections: Option<usize>,
    pub inspector_visible: bool,
    pub default_value: Option<DataValue>,
}

impl Default for InputSlotState {
    fn default() -> Self {
        Self {
            id: InputSlotId::default(),
            label: "",
            data_type: DataType::default(),
            max_connections: None,
            inspector_visible: true,
            default_value: None,
        }
    }
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
    /// Slot-id reverse-owner map: InputSlotId → (owning NodePath, slot_index).
    ///
    /// Kept in lock-step with `input_slots` by every slot insertion/removal path.
    /// Enables O(1) path lookup for edge consumers that only have a slot ID
    /// (plan-006 C17).
    input_slot_owner: HashMap<InputSlotId, (NodePath, usize)>,
    /// Slot-id reverse-owner map: OutputSlotId → (owning NodePath, slot_index).
    output_slot_owner: HashMap<OutputSlotId, (NodePath, usize)>,
    /// All edges in the graph.
    edges: HashMap<EdgeId, Edge>,
    /// Index: input slot → connected edge IDs (ordered).
    input_connections: HashMap<InputSlotId, Vec<EdgeId>>,
    /// Index: output slot → connected edge IDs (ordered).
    output_connections: HashMap<OutputSlotId, Vec<EdgeId>>,
    /// Index: source node → outgoing edge IDs (NodePath-keyed).
    outgoing_edges: HashMap<NodePath, Vec<EdgeId>>,
    /// Index: sink node → incoming edge IDs (NodePath-keyed). The mirror
    /// of `outgoing_edges`; lets "who feeds this node?" be answered in
    /// O(degree) instead of a scan over every input slot in the graph.
    incoming_edges: HashMap<NodePath, Vec<EdgeId>>,
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
            let slot = InputSlotState {
                id: InputSlotId::new(),
                label: def.label,
                data_type: def.data_type,
                max_connections: def.max_connections,
                inspector_visible: def.inspector_visible,
                default_value: None,
            };
            self.input_slot_owner.insert(slot.id, (path.clone(), i));
            self.input_slots.insert((path.clone(), i), slot);
        }

        // Initialize output slot states with metadata
        for (i, def) in output_defs.iter().enumerate() {
            let slot = OutputSlotState {
                id: OutputSlotId::new(),
                label: def.label,
                data_type: def.data_type,
            };
            self.output_slot_owner.insert(slot.id, (path.clone(), i));
            self.output_slots.insert((path.clone(), i), slot);
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
        inspector_visible: bool,
    ) -> usize {
        let index = self.input_slot_count(path);
        let slot = InputSlotState {
            id: InputSlotId::new(),
            label,
            data_type,
            max_connections,
            inspector_visible,
            default_value: None,
        };
        self.input_slot_owner.insert(slot.id, (path.clone(), index));
        self.input_slots.insert((path.clone(), index), slot);
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
        let slot = OutputSlotState {
            id: OutputSlotId::new(),
            label,
            data_type,
        };
        self.output_slot_owner
            .insert(slot.id, (path.clone(), index));
        self.output_slots.insert((path.clone(), index), slot);
        index
    }

    /// Insert a dynamic input slot at `index`, shifting higher-indexed
    /// slots up. Mirror of [`Self::remove_input_slot`], used to restore
    /// a removed slot at its original position (op-log undo).
    ///
    /// Edges referencing this node's input slots at `index` or above
    /// have their `to_input_slot_index` incremented so they keep
    /// pointing at the same logical slot. `index` is clamped to the
    /// current count (clamped insert == append); public `NodeGraph`
    /// wrappers validate the range and return `Err` before calling in.
    pub fn insert_input_slot_at(
        &mut self,
        path: &NodePath,
        index: usize,
        label: &'static str,
        data_type: DataType,
        max_connections: Option<usize>,
        inspector_visible: bool,
    ) -> usize {
        let count = self.input_slot_count(path);
        let index = index.min(count);
        // Shift slots [index..count) up by 1, top-down, updating
        // reverse-owner indices.
        for i in (index..count).rev() {
            if let Some(slot) = self.input_slots.remove(&(path.clone(), i)) {
                self.input_slot_owner.insert(slot.id, (path.clone(), i + 1));
                self.input_slots.insert((path.clone(), i + 1), slot);
            }
        }
        let slot = InputSlotState {
            id: InputSlotId::new(),
            label,
            data_type,
            max_connections,
            inspector_visible,
            default_value: None,
        };
        self.input_slot_owner.insert(slot.id, (path.clone(), index));
        self.input_slots.insert((path.clone(), index), slot);
        // Increment to_input_slot_index on edges referencing shifted
        // slots so indices stay aligned (mirror of remove's decrement).
        for edge in self.edges.values_mut() {
            if edge.to_node == *path && edge.to_input_slot_index >= index {
                edge.to_input_slot_index += 1;
                if let Some(entry) = self.input_slot_owner.get_mut(&edge.to_input_slot_id) {
                    entry.1 = edge.to_input_slot_index;
                }
            }
        }
        index
    }

    /// Insert a dynamic output slot at `index`, shifting higher-indexed
    /// slots up. Mirror of [`Self::remove_output_slot`]; see
    /// [`Self::insert_input_slot_at`] for the shift / edge-remap
    /// contract.
    pub fn insert_output_slot_at(
        &mut self,
        path: &NodePath,
        index: usize,
        label: &'static str,
        data_type: DataType,
    ) -> usize {
        let count = self.output_slot_count(path);
        let index = index.min(count);
        for i in (index..count).rev() {
            if let Some(slot) = self.output_slots.remove(&(path.clone(), i)) {
                self.output_slot_owner
                    .insert(slot.id, (path.clone(), i + 1));
                self.output_slots.insert((path.clone(), i + 1), slot);
            }
        }
        let slot = OutputSlotState {
            id: OutputSlotId::new(),
            label,
            data_type,
        };
        self.output_slot_owner
            .insert(slot.id, (path.clone(), index));
        self.output_slots.insert((path.clone(), index), slot);
        for edge in self.edges.values_mut() {
            if edge.from_node == *path && edge.from_output_slot_index >= index {
                edge.from_output_slot_index += 1;
                if let Some(entry) = self.output_slot_owner.get_mut(&edge.from_output_slot_id) {
                    entry.1 = edge.from_output_slot_index;
                }
            }
        }
        index
    }

    /// Rewrite the `inspector_visible` flag of an input slot. Returns
    /// `true` on success, `false` if the slot does not exist.
    pub fn set_input_slot_inspector_visible(
        &mut self,
        path: &NodePath,
        index: usize,
        visible: bool,
    ) -> bool {
        if let Some(slot) = self.input_slots.get_mut(&(path.clone(), index)) {
            slot.inspector_visible = visible;
            true
        } else {
            false
        }
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
        // Remove the slot at index and tear down its reverse-owner entry.
        if let Some(slot) = self.input_slots.remove(&(path.clone(), index)) {
            self.input_slot_owner.remove(&slot.id);
        }
        // Shift slots above index down by 1, updating reverse-owner indices.
        for i in (index + 1)..count {
            if let Some(slot) = self.input_slots.remove(&(path.clone(), i)) {
                self.input_slot_owner.insert(slot.id, (path.clone(), i - 1));
                self.input_slots.insert((path.clone(), i - 1), slot);
            }
        }
        // Decrement to_input_slot_index on edges that reference this
        // node's higher-indexed input slots so indices stay aligned.
        for edge in self.edges.values_mut() {
            if edge.to_node == *path && edge.to_input_slot_index > index {
                edge.to_input_slot_index -= 1;
                // Update the reverse-owner index for the shifted slot.
                if let Some(entry) = self.input_slot_owner.get_mut(&edge.to_input_slot_id) {
                    entry.1 = edge.to_input_slot_index;
                }
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
        // Remove the slot at index and tear down its reverse-owner entry.
        if let Some(slot) = self.output_slots.remove(&(path.clone(), index)) {
            self.output_slot_owner.remove(&slot.id);
        }
        // Shift slots above index down by 1, updating reverse-owner indices.
        for i in (index + 1)..count {
            if let Some(slot) = self.output_slots.remove(&(path.clone(), i)) {
                self.output_slot_owner
                    .insert(slot.id, (path.clone(), i - 1));
                self.output_slots.insert((path.clone(), i - 1), slot);
            }
        }
        // Decrement from_output_slot_index on edges that reference this
        // node's higher-indexed output slots so indices stay aligned.
        for edge in self.edges.values_mut() {
            if edge.from_node == *path && edge.from_output_slot_index > index {
                edge.from_output_slot_index -= 1;
                // Update the reverse-owner index for the shifted slot.
                if let Some(entry) = self.output_slot_owner.get_mut(&edge.from_output_slot_id) {
                    entry.1 = edge.from_output_slot_index;
                }
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

        // Remove all edges involving this node.
        self.remove_edges_for_node_at(path);

        // Remove slot states and tear down reverse-owner entries.
        let input_ids: Vec<InputSlotId> = self
            .input_slots
            .iter()
            .filter(|((p, _), _)| p == path)
            .map(|(_, s)| s.id)
            .collect();
        for id in input_ids {
            self.input_slot_owner.remove(&id);
        }
        self.input_slots.retain(|(p, _), _| p != path);

        let output_ids: Vec<OutputSlotId> = self
            .output_slots
            .iter()
            .filter(|((p, _), _)| p == path)
            .map(|(_, s)| s.id)
            .collect();
        for id in output_ids {
            self.output_slot_owner.remove(&id);
        }
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

    /// Look up the owning `(NodePath, slot_index)` for an `InputSlotId`.
    ///
    /// Returns `None` if the slot has been removed or was never registered.
    /// Used by downstream edge consumers that only have a slot ID
    /// (plan-006 C17). Exposed as `pub` so P3c.12 modeling-side callers
    /// can use it without touching `NodeStates` internals.
    pub fn input_slot_owner(&self, sid: &InputSlotId) -> Option<&(NodePath, usize)> {
        self.input_slot_owner.get(sid)
    }

    /// Look up the owning `(NodePath, slot_index)` for an `OutputSlotId`.
    ///
    /// Symmetric to `input_slot_owner`. Exposed as `pub` for P3c.12
    /// modeling-side callers (plan-006 C17).
    pub fn output_slot_owner(&self, sid: &OutputSlotId) -> Option<&(NodePath, usize)> {
        self.output_slot_owner.get(sid)
    }

    /// Get all leaf NodeIds (compatibility helper for callers that only need flat IDs).
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().filter_map(|p| p.leaf())
    }

    /// Add an edge and update lookup indexes.
    pub fn add_edge(&mut self, edge_id: EdgeId, edge: Edge) {
        self.changed_nodes.insert(edge.to_node.clone());
        self.input_connections
            .entry(edge.to_input_slot_id)
            .or_default()
            .push(edge_id);
        self.output_connections
            .entry(edge.from_output_slot_id)
            .or_default()
            .push(edge_id);
        self.outgoing_edges
            .entry(edge.from_node.clone())
            .or_default()
            .push(edge_id);
        self.incoming_edges
            .entry(edge.to_node.clone())
            .or_default()
            .push(edge_id);
        self.edges.insert(edge_id, edge);
    }

    /// Remove an edge and update lookup indexes.
    pub fn remove_edge(&mut self, edge_id: &EdgeId) -> Option<Edge> {
        if let Some(edge) = self.edges.remove(edge_id) {
            self.changed_nodes.insert(edge.to_node.clone());
            if let Some(connections) = self.input_connections.get_mut(&edge.to_input_slot_id) {
                connections.retain(|id| id != edge_id);
            }
            if let Some(connections) = self.output_connections.get_mut(&edge.from_output_slot_id) {
                connections.retain(|id| id != edge_id);
            }
            if let Some(outgoing) = self.outgoing_edges.get_mut(&edge.from_node) {
                outgoing.retain(|id| id != edge_id);
            }
            if let Some(incoming) = self.incoming_edges.get_mut(&edge.to_node) {
                incoming.retain(|id| id != edge_id);
            }
            Some(edge)
        } else {
            None
        }
    }

    /// Remove all edges involving a node at `path`, maintaining all indexes.
    pub fn remove_edges_for_node_at(&mut self, path: &NodePath) {
        let edge_ids: Vec<EdgeId> = self
            .edges
            .iter()
            .filter(|(_, edge)| edge.from_node == *path || edge.to_node == *path)
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
        let from_node = edge.from_node.clone();

        let Some(connections) = self.output_connections.get_mut(&slot_id) else {
            return false;
        };
        let Some(old_index) = connections.iter().position(|id| id == edge_id) else {
            return false;
        };

        let id = connections.remove(old_index);
        let insert_at = new_index.min(connections.len());
        connections.insert(insert_at, id);

        self.changed_nodes.insert(from_node);
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
        let to_node = edge.to_node.clone();

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
        self.changed_nodes.insert(to_node);
        true
    }

    /// Get incoming edge IDs into a node.
    pub fn incoming_edges_at(&self, path: &NodePath) -> &[EdgeId] {
        self.incoming_edges
            .get(path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
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

    /// Direct children of a path from the path_index.
    ///
    /// Used by `remove_node_at` to collect descendant paths for
    /// subtree teardown.
    pub(crate) fn path_index_children_of(&self, path: &NodePath) -> &[NodeId] {
        self.path_index.children_of(path)
    }

    /// Return the (label, DataType) pairs visible to the SubGraph's
    /// external callers on its *input* side.
    ///
    /// The input proxy's input slots ARE the external input schema:
    /// wires from outside the SubGraph connect to these slots.
    /// Exposed as `pub(crate)` for P3c.12+ callers (plan-006 Step 10.7).
    #[allow(dead_code)]
    pub(crate) fn subgraph_external_inputs(
        &self,
        sg_path: &NodePath,
        input_proxy_id: NodeId,
    ) -> Vec<(&'static str, DataType)> {
        let in_path = sg_path.child(input_proxy_id);
        let n = self.input_slot_count(&in_path);
        (0..n)
            .filter_map(|i| self.input_slot(&in_path, i).map(|s| (s.label, s.data_type)))
            .collect()
    }

    /// Return the (label, DataType) pairs visible to the SubGraph's
    /// external callers on its *output* side.
    ///
    /// The output proxy's *input* slots ARE the external output schema:
    /// the proxy receives data from the SubGraph's internal nodes on
    /// these slots and tees it out to external consumers.
    /// Exposed as `pub(crate)` for P3c.12+ callers (plan-006 Step 10.7).
    #[allow(dead_code)]
    pub(crate) fn subgraph_external_outputs(
        &self,
        sg_path: &NodePath,
        output_proxy_id: NodeId,
    ) -> Vec<(&'static str, DataType)> {
        let out_path = sg_path.child(output_proxy_id);
        let n = self.input_slot_count(&out_path);
        (0..n)
            .filter_map(|i| {
                self.input_slot(&out_path, i)
                    .map(|s| (s.label, s.data_type))
            })
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    /// Verify that `subgraph_external_inputs` returns the input proxy's
    /// input slots.
    #[test]
    fn subgraph_external_inputs_returns_proxy_input_slots() {
        let mut ns = NodeStates::new();
        let sg_id = NodeId::new();
        let in_id = NodeId::new();
        let sg_path = NodePath::root().child(sg_id);
        let in_path = sg_path.child(in_id);

        ns.add_node(&sg_path, "SubGraph", None, None, &[], &[]);
        ns.add_node(&in_path, "Interface", None, None, &[], &[]);
        ns.add_input_slot(&in_path, "value", DataType::Number, Some(1), true);

        let schema = ns.subgraph_external_inputs(&sg_path, in_id);
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].0, "value");
        assert_eq!(schema[0].1, DataType::Number);
    }

    /// Verify that `subgraph_external_outputs` reads the output proxy's
    /// *input* slots (the external output schema).
    #[test]
    fn subgraph_external_outputs_returns_output_proxy_input_slots() {
        let mut ns = NodeStates::new();
        let sg_id = NodeId::new();
        let out_id = NodeId::new();
        let sg_path = NodePath::root().child(sg_id);
        let out_path = sg_path.child(out_id);

        ns.add_node(&sg_path, "SubGraph", None, None, &[], &[]);
        ns.add_node(&out_path, "Interface", None, None, &[], &[]);
        ns.add_input_slot(&out_path, "result", DataType::Number, None, true);

        let schema = ns.subgraph_external_outputs(&sg_path, out_id);
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].0, "result");
    }

    /// Verify that `input_slot_owner` and `output_slot_owner` reverse maps
    /// are populated on slot insertion and torn down on slot removal.
    #[test]
    fn slot_owner_reverse_maps_populated_and_torn_down() {
        let mut ns = NodeStates::new();
        let node_id = NodeId::new();
        let path = NodePath::root().child(node_id);

        ns.add_node(&path, "Test", None, None, &[], &[]);

        // Add one input slot and one output slot dynamically.
        let in_idx = ns.add_input_slot(&path, "in", DataType::Number, Some(1), true);
        let out_idx = ns.add_output_slot(&path, "out", DataType::Number);

        let in_sid = ns.input_slot(&path, in_idx).unwrap().id;
        let out_sid = ns.output_slot(&path, out_idx).unwrap().id;

        // Reverse maps must be populated.
        let (in_owner_path, in_owner_idx) = ns.input_slot_owner(&in_sid).unwrap();
        assert_eq!(*in_owner_path, path);
        assert_eq!(*in_owner_idx, in_idx);

        let (out_owner_path, out_owner_idx) = ns.output_slot_owner(&out_sid).unwrap();
        assert_eq!(*out_owner_path, path);
        assert_eq!(*out_owner_idx, out_idx);

        // Remove the input slot; the reverse entry must be gone.
        ns.remove_input_slot(&path, in_idx);
        assert!(ns.input_slot_owner(&in_sid).is_none());

        // Remove the output slot; the reverse entry must be gone.
        ns.remove_output_slot(&path, out_idx);
        assert!(ns.output_slot_owner(&out_sid).is_none());
    }

    /// Verify that static slot defs (via `add_node`) also populate reverse maps.
    #[test]
    fn slot_owner_populated_from_static_defs() {
        use crate::SlotDef;
        let mut ns = NodeStates::new();
        let node_id = NodeId::new();
        let path = NodePath::root().child(node_id);

        let input_defs = [SlotDef {
            label: "value",
            data_type: DataType::Number,
            max_connections: Some(1),
            inspector_visible: true,
        }];
        let output_defs = [SlotDef {
            label: "result",
            data_type: DataType::Number,
            max_connections: None,
            inspector_visible: true,
        }];

        ns.add_node(&path, "Test", None, None, &input_defs, &output_defs);

        let in_sid = ns.input_slot(&path, 0).unwrap().id;
        let out_sid = ns.output_slot(&path, 0).unwrap().id;

        assert!(ns.input_slot_owner(&in_sid).is_some());
        assert!(ns.output_slot_owner(&out_sid).is_some());

        // Verify contents.
        let (p, i) = ns.input_slot_owner(&in_sid).unwrap();
        assert_eq!(*p, path);
        assert_eq!(*i, 0);
        let (p, i) = ns.output_slot_owner(&out_sid).unwrap();
        assert_eq!(*p, path);
        assert_eq!(*i, 0);
    }

    /// Verify that removing a node tears down reverse-owner entries for
    /// all its slots.
    #[test]
    fn remove_node_clears_slot_owner_maps() {
        let mut ns = NodeStates::new();
        let node_id = NodeId::new();
        let path = NodePath::root().child(node_id);

        ns.add_node(&path, "Test", None, None, &[], &[]);
        ns.add_input_slot(&path, "i", DataType::Number, None, true);
        ns.add_output_slot(&path, "o", DataType::Number);

        let in_sid = ns.input_slot(&path, 0).unwrap().id;
        let out_sid = ns.output_slot(&path, 0).unwrap().id;

        ns.remove_node(&path);

        assert!(ns.input_slot_owner(&in_sid).is_none());
        assert!(ns.output_slot_owner(&out_sid).is_none());
    }
}
