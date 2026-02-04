//! External state access trait for decoupled state management.
//!
//! This module provides the `NodeStatesAccess` trait which allows cognet
//! to delegate state management to external consumers (like revion).
//!
//! # Design Philosophy
//!
//! cognet is a graph computation engine that should remain independent of
//! UI frameworks. However, UI systems need to observe graph state changes
//! for rendering. This trait provides a clean abstraction:
//!
//! - cognet defines what state operations are needed
//! - UI frameworks implement how to store and notify about changes
//!
//! # Fine-grained API
//!
//! The trait provides individual methods for each type of mutation,
//! allowing implementations to:
//! - Track exactly what changed (for incremental rendering)
//! - Trigger notifications only when needed
//! - Optimize for their specific use case

use crate::{Data, Edge, EdgeId, NodeId, NodeStates};
use std::sync::{Arc, RwLock};

/// Trait for external state access with fine-grained updates.
///
/// cognet is independent - consumers provide their own implementation.
///
/// # Example Implementation
///
/// ```ignore
/// struct MyStateAdapter {
///     storage: Arc<RwLock<NodeStates>>,
///     on_change: Box<dyn Fn() + Send + Sync>,
/// }
///
/// impl NodeStatesAccess for MyStateAdapter {
///     fn storage(&self) -> &Arc<RwLock<NodeStates>> {
///         &self.storage
///     }
///
///     fn add_node(&self, node_id: NodeId, ...) {
///         self.storage.write().unwrap().add_node(...);
///         (self.on_change)();
///     }
///     // ... implement other methods
/// }
/// ```
pub trait NodeStatesAccess: Send + Sync {
    // ========================================================================
    // Read Operations
    // ========================================================================

    /// Get the underlying storage for direct read access.
    ///
    /// This provides access to the Arc<RwLock<NodeStates>> for reading.
    /// Use `storage().read()` to get a read guard.
    fn storage(&self) -> &Arc<RwLock<NodeStates>>;

    // ========================================================================
    // Node Operations
    // ========================================================================

    /// Add a node with its initial state.
    ///
    /// This should be called after the node is created in NodeManager.
    fn add_node(
        &self,
        node_id: NodeId,
        type_name: &'static str,
        data: Option<Data>,
        input_count: usize,
        output_count: usize,
    );

    /// Remove a node and its slot states.
    fn remove_node(&self, node_id: NodeId);

    /// Update a node's data value.
    fn update_node_data(&self, node_id: NodeId, data: Option<Data>);

    // ========================================================================
    // Edge Operations
    // ========================================================================

    /// Add an edge.
    fn add_edge(&self, edge_id: EdgeId, edge: Edge);

    /// Remove an edge.
    fn remove_edge(&self, edge_id: EdgeId);

    // ========================================================================
    // Slot Operations
    // ========================================================================

    /// Update input slot's connected edges.
    fn update_input_slot_edges(&self, node_id: NodeId, slot_index: usize, edges: Vec<EdgeId>);

    /// Update output slot's connected edges.
    fn update_output_slot_edges(&self, node_id: NodeId, slot_index: usize, edges: Vec<EdgeId>);
}

/// A simple implementation that operates directly on NodeStates.
///
/// This is used when no external state access is provided (standalone mode).
pub struct InternalStateAccess {
    states: Arc<RwLock<NodeStates>>,
}

impl InternalStateAccess {
    /// Create a new internal state access.
    pub fn new() -> Self {
        Self {
            states: Arc::new(RwLock::new(NodeStates::new())),
        }
    }
}

impl Default for InternalStateAccess {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeStatesAccess for InternalStateAccess {
    fn storage(&self) -> &Arc<RwLock<NodeStates>> {
        &self.states
    }

    fn add_node(
        &self,
        node_id: NodeId,
        type_name: &'static str,
        data: Option<Data>,
        input_count: usize,
        output_count: usize,
    ) {
        let mut guard = self.states.write().expect("lock poisoned");
        guard.add_node(node_id, type_name, data, input_count, output_count);
    }

    fn remove_node(&self, node_id: NodeId) {
        let mut guard = self.states.write().expect("lock poisoned");
        guard.remove_node(&node_id);
    }

    fn update_node_data(&self, node_id: NodeId, data: Option<Data>) {
        let mut guard = self.states.write().expect("lock poisoned");
        if let Some(node) = guard.get_mut(&node_id) {
            node.data = data;
        }
    }

    fn add_edge(&self, edge_id: EdgeId, edge: Edge) {
        let mut guard = self.states.write().expect("lock poisoned");
        guard.add_edge(edge_id, edge);
    }

    fn remove_edge(&self, edge_id: EdgeId) {
        let mut guard = self.states.write().expect("lock poisoned");
        guard.remove_edge(&edge_id);
    }

    fn update_input_slot_edges(&self, node_id: NodeId, slot_index: usize, edges: Vec<EdgeId>) {
        let mut guard = self.states.write().expect("lock poisoned");
        if let Some(slot) = guard.input_slot_mut(&node_id, slot_index) {
            slot.connected_edges = edges;
        }
    }

    fn update_output_slot_edges(&self, node_id: NodeId, slot_index: usize, edges: Vec<EdgeId>) {
        let mut guard = self.states.write().expect("lock poisoned");
        if let Some(slot) = guard.output_slot_mut(&node_id, slot_index) {
            slot.connected_edges = edges;
        }
    }
}
