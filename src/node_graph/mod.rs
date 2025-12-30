pub mod execution_cache;
pub mod node_graph_api;
pub mod node_graph_system;
pub mod node_manager;
pub mod node_state;

pub use execution_cache::*;
pub use node_graph_api::*;
pub use node_manager::*;
pub use node_state::*;

use crate::{ErrorTarget, GraphError, NodeId};
use std::collections::{HashMap, HashSet};

pub struct NodeGraph {
    node_manager: NodeManager,
    node_states: SharedNodeStates,
    cache: SharedExecutionCache,
    dirty_nodes: HashSet<NodeId>,
    /// Current errors in the graph, keyed by target.
    errors: HashMap<ErrorTarget, GraphError>,
}

impl NodeGraph {
    /// Get shared node states (for sync UI access).
    ///
    /// This returns a clone of the Arc, allowing external code
    /// to access node states without holding the graph lock.
    pub fn shared_node_states(&self) -> SharedNodeStates {
        self.node_states.share()
    }

    /// Get shared execution cache (for UI access to output values).
    pub fn shared_cache(&self) -> SharedExecutionCache {
        self.cache.share()
    }

    /// Get all current errors.
    pub fn errors(&self) -> &HashMap<ErrorTarget, GraphError> {
        &self.errors
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get error for a specific target.
    pub fn error_for(&self, target: &ErrorTarget) -> Option<&GraphError> {
        self.errors.get(target)
    }

    /// Add an error.
    pub fn add_error(&mut self, error: GraphError) {
        self.errors.insert(error.target.clone(), error);
    }

    /// Clear error for a specific target.
    pub fn clear_error(&mut self, target: &ErrorTarget) {
        self.errors.remove(target);
    }

    /// Clear all errors.
    pub fn clear_all_errors(&mut self) {
        self.errors.clear();
    }

    /// Clear all execution errors (before running execute()).
    pub fn clear_execution_errors(&mut self) {
        self.errors.retain(|_, e| !e.is_execution_error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute() {}
}
