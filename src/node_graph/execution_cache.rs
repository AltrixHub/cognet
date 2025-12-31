use crate::{Data, Edge, EdgeId, InputSlotId, OutputSlotId};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Default, Clone)]
pub struct SharedExecutionCache {
    inner: Arc<Mutex<ExecutionCache>>,
}

impl SharedExecutionCache {
    pub fn new(cache: ExecutionCache) -> Self {
        SharedExecutionCache {
            inner: Arc::new(Mutex::new(cache)),
        }
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, ExecutionCache>, String> {
        Ok(self.inner.lock().map_err(|_| "Failed to lock cache")?)
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
}

impl ExecutionCache {
    /// Add an edge and update the input connection index.
    pub fn add_edge(&mut self, edge_id: EdgeId, edge: Edge) {
        let input_slot_id = edge.to_input_slot_id;
        self.edges.insert(edge_id, edge);
        self.input_connections
            .entry(input_slot_id)
            .or_default()
            .push(edge_id);
    }

    /// Remove an edge and update the input connection index.
    pub fn remove_edge(&mut self, edge_id: &EdgeId) -> Option<Edge> {
        if let Some(edge) = self.edges.remove(edge_id) {
            if let Some(connections) = self.input_connections.get_mut(&edge.to_input_slot_id) {
                connections.retain(|id| id != edge_id);
            }
            Some(edge)
        } else {
            None
        }
    }

    /// Get edges connected to an input slot.
    pub fn edges_for_input(&self, input_slot_id: &InputSlotId) -> &[EdgeId] {
        self.input_connections
            .get(input_slot_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get output value by slot ID.
    pub fn get_output(&self, output_slot_id: &OutputSlotId) -> Option<&Data> {
        self.outputs.get(output_slot_id)
    }
}
