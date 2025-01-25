pub mod api;
pub mod system;

pub use api::*;
pub use system::*;

use std::collections::{HashMap, HashSet};

use crate::{Data, Edge, EdgeId, NodeId, NodeImpl, OutputSlotId};

#[derive(Default, Debug)]
pub struct EvaluationContext {
    pub(crate) edges: HashMap<EdgeId, Edge>,
    pub(crate) outputs: HashMap<OutputSlotId, Data>,
}

#[derive(Default, Debug)]
pub struct NodeGraph {
    pub nodes: HashMap<NodeId, Box<dyn NodeImpl>>,
    pub context: EvaluationContext,
    pub dirty_nodes: HashSet<NodeId>,
}

#[cfg(test)]
mod tests {
    use crate::{AddListNode, NodePrimitive, NumberNode};

    use super::*;

    #[test]
    fn test_execute() {
        let mut node_graph = NodeGraph::new();

        let node1 = NumberNode::initialize();
        node1
            .set_default_value(&mut node_graph.context, Data::Number(10.))
            .unwrap();
        let node2 = NumberNode::initialize();
        node2
            .set_default_value(&mut node_graph.context, Data::Number(20.))
            .unwrap();
        let node3 = AddListNode::initialize();

        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);
        let node3_id = node_graph.add_node(node3);

        node_graph.connect_nodes(node1_id, 0, node3_id, 0).unwrap();
        node_graph.connect_nodes(node2_id, 0, node3_id, 0).unwrap();

        assert!(node_graph.execute().is_ok());

        let res = node_graph.get_output_value(node3_id, 0).unwrap();
        assert_eq!(res, &Data::Number(30.));
    }

    #[test]
    fn test_remove_node() {
        let mut node_graph = NodeGraph::new();
        let node = AddListNode::initialize();
        let node_id = node_graph.add_node(node);

        assert!(node_graph.remove_node(node_id).is_ok());
        assert!(node_graph.get_node_by_id(node_id).is_none());
    }

    #[test]
    fn test_remove_edge() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddListNode::initialize();
        let node2 = AddListNode::initialize();
        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);

        let edge_id = node_graph.connect_nodes(node1_id, 0, node2_id, 0).unwrap();

        assert!(node_graph.remove_edge(edge_id).is_ok());
        assert!(!node_graph.context.edges.contains_key(&edge_id));
    }

    #[test]
    fn test_node_execution() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddListNode::initialize();
        let node2 = AddListNode::initialize();
        let node3 = AddListNode::initialize();

        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);
        let node3_id = node_graph.add_node(node3);

        node_graph.connect_nodes(node1_id, 0, node2_id, 0).unwrap();
        node_graph.connect_nodes(node2_id, 0, node3_id, 0).unwrap();

        node_graph.execute().unwrap();

        println!("--- first execute finished---");

        node_graph
            .update_node(node2_id, AddListNode::initialize())
            .unwrap();

        node_graph.execute().unwrap();
    }
}
