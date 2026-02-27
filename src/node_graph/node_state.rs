//! Runtime state for node instances.

use crate::{Data, Edge, EdgeId, InputSlotId, NodeId, OutputSlotId};
use std::collections::HashMap;

/// Runtime state for a single node instance.
#[derive(Debug, Clone)]
pub struct NodeState {
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
pub struct InputSlotState {
    pub id: InputSlotId,
    pub connected_edges: Vec<EdgeId>,
}

/// Runtime state for output slots.
#[derive(Debug, Default, Clone)]
pub struct OutputSlotState {
    pub id: OutputSlotId,
    pub connected_edges: Vec<EdgeId>,
}

/// Manages all node states in the graph.
#[derive(Debug, Default, Clone)]
pub struct NodeStates {
    /// Node data by NodeId.
    nodes: HashMap<NodeId, NodeState>,
    /// Input slot states by (NodeId, slot_index).
    input_slots: HashMap<(NodeId, usize), InputSlotState>,
    /// Output slot states by (NodeId, slot_index).
    output_slots: HashMap<(NodeId, usize), OutputSlotState>,
    /// All edges in the graph.
    edges: HashMap<EdgeId, Edge>,
}

impl NodeStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new node.
    pub fn add_node(
        &mut self,
        node_id: NodeId,
        type_name: &'static str,
        default_data: Option<Data>,
        input_count: usize,
        output_count: usize,
    ) {
        self.nodes
            .insert(node_id, NodeState::new(type_name, default_data));

        // Initialize input slot states
        for i in 0..input_count {
            self.input_slots.insert(
                (node_id, i),
                InputSlotState {
                    id: InputSlotId::new(),
                    connected_edges: Vec::new(),
                },
            );
        }

        // Initialize output slot states
        for i in 0..output_count {
            self.output_slots.insert(
                (node_id, i),
                OutputSlotState {
                    id: OutputSlotId::new(),
                    connected_edges: Vec::new(),
                },
            );
        }
    }

    /// Remove a node and its slot states.
    pub fn remove_node(&mut self, node_id: &NodeId) -> Option<NodeState> {
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

    /// Get mutable output slot state.
    pub fn output_slot_mut(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
    ) -> Option<&mut OutputSlotState> {
        self.output_slots.get_mut(&(*node_id, slot_index))
    }

    /// Get all node IDs.
    pub fn node_ids(&self) -> impl Iterator<Item = &NodeId> {
        self.nodes.keys()
    }

    /// Get all nodes.
    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &NodeState)> {
        self.nodes.iter()
    }

    /// Add an edge.
    pub fn add_edge(&mut self, edge_id: EdgeId, edge: Edge) {
        self.edges.insert(edge_id, edge);
    }

    /// Remove an edge.
    pub fn remove_edge(&mut self, edge_id: &EdgeId) -> Option<Edge> {
        self.edges.remove(edge_id)
    }

    /// Get an edge by ID.
    pub fn get_edge(&self, edge_id: &EdgeId) -> Option<&Edge> {
        self.edges.get(edge_id)
    }

    /// Get all edges.
    pub fn edges(&self) -> &HashMap<EdgeId, Edge> {
        &self.edges
    }

    /// Get edges connected to a node.
    pub fn edges_for_node(&self, node_id: &NodeId) -> Vec<(&EdgeId, &Edge)> {
        self.edges
            .iter()
            .filter(|(_, e)| &e.from_node_id == node_id || &e.to_node_id == node_id)
            .collect()
    }
}
