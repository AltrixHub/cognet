pub mod evaluation_context;
pub mod node_graph_api;
pub mod node_graph_system;
pub mod node_manager;

pub use evaluation_context::*;
pub use node_graph_api::*;
pub use node_manager::*;

use crate::NodeId;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

pub struct NodeGraph {
    node_manager: NodeManager,
    context: Arc<Mutex<EvaluationContext>>,
    dirty_nodes: HashSet<NodeId>,
}

impl Default for NodeGraph {
    fn default() -> Self {
        Self {
            node_manager: NodeManager::new(),
            context: Default::default(),
            dirty_nodes: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute() {}
}
