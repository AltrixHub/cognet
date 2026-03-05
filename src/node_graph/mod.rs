pub mod convenience;
pub mod edge_info;
pub mod execution_cache;
pub mod node_graph_api;
pub mod node_graph_system;
pub mod node_manager;
pub mod node_state;
pub mod path_navigation;
pub mod subgraph_helpers;
pub mod subgraph_ops;

pub use edge_info::*;
pub use execution_cache::*;
pub use node_graph_api::*;
pub use node_manager::*;
pub use node_state::*;
pub use subgraph_ops::*;

use crate::{ErrorTarget, GraphError, NodeId};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

/// Mutable bookkeeping state used during execution and graph mutation.
///
/// Wrapped in `Mutex` for interior mutability so that `execute()` can
/// take `&self` instead of `&mut self`, eliminating the need for the
/// app layer to hold a write lock on `NodeGraph` during execution.
#[derive(Default)]
struct Bookkeeping {
    dirty_nodes: HashSet<NodeId>,
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
    pub fn node_states(&self) -> &Arc<RwLock<NodeStates>> {
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
        let all_ids: Vec<NodeId> = self.node_manager.all_node_ids();
        if let Ok(mut b) = self.bookkeeping.lock() {
            b.dirty_nodes.extend(all_ids);
        }
    }

    /// Mark specific nodes as dirty.
    pub(crate) fn mark_dirty(&self, nodes: impl IntoIterator<Item = NodeId>) {
        if let Ok(mut b) = self.bookkeeping.lock() {
            b.dirty_nodes.extend(nodes);
        }
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
