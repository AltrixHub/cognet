pub mod execution_cache;
pub mod node_graph_api;
pub mod node_graph_system;
pub mod node_manager;

pub use execution_cache::*;
pub use node_graph_api::*;
pub use node_manager::*;

use crate::NodeId;
use std::collections::HashSet;

pub struct NodeGraph {
    node_manager: NodeManager,
    cache: SharedExecutionCache,
    dirty_nodes: HashSet<NodeId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute() {}
}
