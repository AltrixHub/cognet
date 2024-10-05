use std::collections::HashMap;

pub mod edge;
use derive_new::new;
pub use edge::*;

#[derive(Debug, new, Clone)]
pub struct Node {
    pub entity: String,
}

pub type Nodes = HashMap<String, Node>;
pub type Edges = HashMap<String, Edge>;

#[derive(Default, Debug)]
pub struct EvaluationContext {
    inputs: HashMap<String, Vec<InputSlot>>,
    outputs: HashMap<String, Vec<OutputSlot>>,
}

#[derive(Default, Debug, new)]
pub struct NodeGraph {
    pub nodes: Nodes,
    pub edges: Edges,
    pub context: EvaluationContext,
}

impl NodeGraph {
    fn execute() {
        unimplemented!()
    }

    fn topological_sort() {
        unimplemented!()
    }

    fn add_node() {
        unimplemented!()
    }

    fn remove_node() {
        unimplemented!()
    }

    fn update_node() {
        unimplemented!()
    }

    fn get_node_by_id() {
        unimplemented!()
    }

    fn get_nodes_by_type() {
        unimplemented!()
    }

    fn get_all_nodes() {
        unimplemented!()
    }

    fn add_edge() {
        unimplemented!()
    }

    fn remove_edge() {
        unimplemented!()
    }

    fn update_edge() {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_sort() {
        let node_graph = NodeGraph::default();
        unimplemented!()
    }

    #[test]
    fn add_node() {
        let node_graph = NodeGraph::default();
        unimplemented!()
    }

    #[test]
    fn remove_node() {
        let node_graph = NodeGraph::default();
        unimplemented!()
    }

    #[test]
    fn update_node() {
        let node_graph = NodeGraph::default();
        unimplemented!()
    }
}
