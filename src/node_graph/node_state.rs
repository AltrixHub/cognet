//! Runtime state for node instances.

use crate::{Data, DataType, DataValue, Edge, EdgeId, InputSlotId, NodeId, OutputSlotId, SlotDef};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Runtime state for a single node instance.
#[derive(Debug, Clone)]
pub(crate) struct NodeState {
    /// The node type name (references NodeMeta::NAME).
    pub type_name: &'static str,
    /// Current data value (can be modified at runtime).
    pub data: Option<Data>,
}

impl NodeState {
    pub fn new(type_name: &'static str, default_data: Option<Data>) -> Self {
        Self {
            type_name,
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
    /// Node data by NodeId.
    nodes: HashMap<NodeId, NodeState>,
    /// Input slot states by (NodeId, slot_index).
    input_slots: HashMap<(NodeId, usize), InputSlotState>,
    /// Output slot states by (NodeId, slot_index).
    output_slots: HashMap<(NodeId, usize), OutputSlotState>,
    /// All edges in the graph.
    edges: HashMap<EdgeId, Edge>,
    /// Index: input slot → connected edge IDs.
    input_connections: HashMap<InputSlotId, Vec<EdgeId>>,
    /// Index: source node → outgoing edge IDs.
    outgoing_edges: HashMap<NodeId, Vec<EdgeId>>,
    /// Nodes changed since last drain (for deferred dirty tracking).
    changed_nodes: HashSet<NodeId>,
}

impl NodeStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new node with slot metadata from `SlotDef` arrays.
    pub fn add_node(
        &mut self,
        node_id: NodeId,
        type_name: &'static str,
        default_data: Option<Data>,
        input_defs: &[SlotDef],
        output_defs: &[SlotDef],
    ) {
        self.nodes
            .insert(node_id, NodeState::new(type_name, default_data));

        // Initialize input slot states with metadata
        for (i, def) in input_defs.iter().enumerate() {
            self.input_slots.insert(
                (node_id, i),
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
                (node_id, i),
                OutputSlotState {
                    id: OutputSlotId::new(),
                    label: def.label,
                    data_type: def.data_type,
                },
            );
        }

        self.changed_nodes.insert(node_id);
    }

    /// Add a dynamic input slot (for SubGraphNode).
    pub fn add_input_slot(
        &mut self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
        max_connections: Option<usize>,
    ) -> usize {
        let index = self.input_slot_count(node_id);
        self.input_slots.insert(
            (*node_id, index),
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
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> usize {
        let index = self.output_slot_count(node_id);
        self.output_slots.insert(
            (*node_id, index),
            OutputSlotState {
                id: OutputSlotId::new(),
                label,
                data_type,
            },
        );
        index
    }

    /// Remove a dynamic input slot by index and shift higher-indexed slots down.
    pub fn remove_input_slot(&mut self, node_id: &NodeId, index: usize) {
        let count = self.input_slot_count(node_id);
        if index >= count {
            return;
        }
        // Remove the slot at index
        self.input_slots.remove(&(*node_id, index));
        // Shift slots above index down by 1
        for i in (index + 1)..count {
            if let Some(slot) = self.input_slots.remove(&(*node_id, i)) {
                self.input_slots.insert((*node_id, i - 1), slot);
            }
        }
    }

    /// Remove a dynamic output slot by index and shift higher-indexed slots down.
    pub fn remove_output_slot(&mut self, node_id: &NodeId, index: usize) {
        let count = self.output_slot_count(node_id);
        if index >= count {
            return;
        }
        // Remove the slot at index
        self.output_slots.remove(&(*node_id, index));
        // Shift slots above index down by 1
        for i in (index + 1)..count {
            if let Some(slot) = self.output_slots.remove(&(*node_id, i)) {
                self.output_slots.insert((*node_id, i - 1), slot);
            }
        }
    }

    /// Count input slots for a node.
    pub fn input_slot_count(&self, node_id: &NodeId) -> usize {
        self.input_slots
            .keys()
            .filter(|(id, _)| id == node_id)
            .count()
    }

    /// Count output slots for a node.
    pub fn output_slot_count(&self, node_id: &NodeId) -> usize {
        self.output_slots
            .keys()
            .filter(|(id, _)| id == node_id)
            .count()
    }

    /// Remove a node, its slot states, and all connected edges.
    pub fn remove_node(&mut self, node_id: &NodeId) -> Option<NodeState> {
        // Remove all edges involving this node (maintains indexes)
        self.remove_edges_for_node(node_id);

        // Remove slot states
        self.input_slots.retain(|(id, _), _| id != node_id);
        self.output_slots.retain(|(id, _), _| id != node_id);

        self.nodes.remove(node_id)
    }

    /// Get node state.
    pub fn get(&self, node_id: &NodeId) -> Option<&NodeState> {
        self.nodes.get(node_id)
    }

    /// Get mutable node state.
    pub fn get_mut(&mut self, node_id: &NodeId) -> Option<&mut NodeState> {
        self.nodes.get_mut(node_id)
    }

    /// Get input slot state.
    pub fn input_slot(&self, node_id: &NodeId, slot_index: usize) -> Option<&InputSlotState> {
        self.input_slots.get(&(*node_id, slot_index))
    }

    /// Get mutable input slot state.
    pub fn input_slot_mut(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
    ) -> Option<&mut InputSlotState> {
        self.input_slots.get_mut(&(*node_id, slot_index))
    }

    /// Get output slot state.
    pub fn output_slot(&self, node_id: &NodeId, slot_index: usize) -> Option<&OutputSlotState> {
        self.output_slots.get(&(*node_id, slot_index))
    }

    /// Get all node IDs.
    pub fn node_ids(&self) -> impl Iterator<Item = &NodeId> {
        self.nodes.keys()
    }

    /// Add an edge and update lookup indexes.
    pub fn add_edge(&mut self, edge_id: EdgeId, edge: Edge) {
        self.changed_nodes.insert(edge.to_node_id);
        self.input_connections
            .entry(edge.to_input_slot_id)
            .or_default()
            .push(edge_id);
        self.outgoing_edges
            .entry(edge.from_node_id)
            .or_default()
            .push(edge_id);
        self.edges.insert(edge_id, edge);
    }

    /// Remove an edge and update lookup indexes.
    pub fn remove_edge(&mut self, edge_id: &EdgeId) -> Option<Edge> {
        if let Some(edge) = self.edges.remove(edge_id) {
            self.changed_nodes.insert(edge.to_node_id);
            if let Some(connections) = self.input_connections.get_mut(&edge.to_input_slot_id) {
                connections.retain(|id| id != edge_id);
            }
            if let Some(outgoing) = self.outgoing_edges.get_mut(&edge.from_node_id) {
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

    /// Get edges connected to an input slot.
    pub fn edges_for_input(&self, input_slot_id: &InputSlotId) -> &[EdgeId] {
        self.input_connections
            .get(input_slot_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get outgoing edge IDs from a node.
    pub fn outgoing_edges_for_node(&self, node_id: &NodeId) -> &[EdgeId] {
        self.outgoing_edges
            .get(node_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Mark a node as changed (needs re-execution).
    pub fn mark_changed(&mut self, node_id: NodeId) {
        self.changed_nodes.insert(node_id);
    }

    /// Mark all nodes as changed (forces full re-execution).
    pub fn mark_all_changed(&mut self) {
        self.changed_nodes.extend(self.nodes.keys().copied());
    }

    /// Drain and return all changed node IDs since last drain.
    pub fn drain_changed_nodes(&mut self) -> HashSet<NodeId> {
        std::mem::take(&mut self.changed_nodes)
    }
}
