use crate::{Data, DataType, Edge, EdgeId, InputSlotId, NodeId, OutputSlotId};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

#[derive(Default, Clone)]
pub struct SharedExecutionCache {
    inner: Arc<RwLock<ExecutionCache>>,
}

impl SharedExecutionCache {
    pub fn new(cache: ExecutionCache) -> Self {
        SharedExecutionCache {
            inner: Arc::new(RwLock::new(cache)),
        }
    }

    /// Acquire a write lock (backward-compatible with existing callers).
    pub fn lock(&self) -> Result<RwLockWriteGuard<'_, ExecutionCache>, String> {
        self.inner
            .write()
            .map_err(|_| "Failed to lock cache".to_string())
    }

    /// Acquire a read lock for read-only access.
    pub fn read(&self) -> Result<RwLockReadGuard<'_, ExecutionCache>, String> {
        self.inner
            .read()
            .map_err(|_| "Failed to read cache".to_string())
    }

    pub fn share(&self) -> Self {
        SharedExecutionCache {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[derive(Default, Debug)]
pub struct ExecutionCache {
    pub(crate) edges: HashMap<EdgeId, Edge>,
    pub(crate) outputs: HashMap<OutputSlotId, Data>,
    /// Index for fast lookup of edges by input slot.
    pub(crate) input_connections: HashMap<InputSlotId, Vec<EdgeId>>,
    /// Index for fast lookup of outgoing edges by source node.
    pub(crate) outgoing_edges: HashMap<NodeId, Vec<EdgeId>>,
}

impl ExecutionCache {
    /// Add an edge and update all connection indexes.
    pub fn add_edge(&mut self, edge_id: EdgeId, edge: Edge) {
        let input_slot_id = edge.to_input_slot_id;
        let from_node_id = edge.from_node_id;
        self.edges.insert(edge_id, edge);
        self.input_connections
            .entry(input_slot_id)
            .or_default()
            .push(edge_id);
        self.outgoing_edges
            .entry(from_node_id)
            .or_default()
            .push(edge_id);
    }

    /// Remove an edge and update all connection indexes.
    pub fn remove_edge(&mut self, edge_id: &EdgeId) -> Option<Edge> {
        if let Some(edge) = self.edges.remove(edge_id) {
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

    /// Get output value by slot ID.
    pub fn get_output(&self, output_slot_id: &OutputSlotId) -> Option<&Data> {
        self.outputs.get(output_slot_id)
    }

    /// Remove all `DataType::Mesh` outputs from the cache.
    ///
    /// Frees `Arc<dyn Any>` payloads (typically containing mesh vertex/index data)
    /// while preserving Number and String outputs needed for edge value display
    /// and incremental re-execution.
    ///
    /// Call this after the application layer has consumed mesh data (e.g., uploaded
    /// to GPU) and no longer needs the cached copies.
    pub fn evict_mesh_outputs(&mut self) -> usize {
        let before = self.outputs.len();
        self.outputs.retain(|_, data| data.get_type() != DataType::Mesh);
        before - self.outputs.len()
    }
}
