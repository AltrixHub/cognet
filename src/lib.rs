use std::{
    any::Any,
    collections::{HashMap, VecDeque},
    fmt::Debug,
};

pub mod edge;
pub use edge::*;
use ulid::Ulid;

pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub trait NodeEntity: Debug + AsAny {
    fn execute(&self);
}

impl<T: 'static + NodeEntity> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub type NodeId = Ulid;
pub type EdgeId = Ulid;

pub type Edges = HashMap<String, Edge>;

#[derive(Default, Debug)]
pub struct EvaluationContext {
    inputs: HashMap<InputSlotId, Vec<InputSlot>>,
    outputs: HashMap<OutputSlotId, Vec<OutputSlot>>,
}

#[derive(Default, Debug)]
pub struct NodeGraph {
    pub nodes: HashMap<NodeId, Box<dyn NodeEntity>>,
    pub edges: HashMap<EdgeId, Edge>,
    pub context: EvaluationContext,
}

impl NodeGraph {
    fn new() -> Self {
        Self::default()
    }

    pub fn execute(&self) -> Result<(), &'static str> {
        let sorted_nodes = self.topological_sort()?;
        for node_id in sorted_nodes {
            if let Some(node) = self.nodes.get(&node_id) {
                node.execute();
            }
        }
        Ok(())
    }

    pub fn topological_sort(&self) -> Result<Vec<NodeId>, &'static str> {
        let mut in_degree = HashMap::new();
        let mut adj_list = HashMap::new();

        for (&node_id, _) in &self.nodes {
            in_degree.insert(node_id, 0);
            adj_list.insert(node_id, Vec::new());
        }

        for edge in self.edges.values() {
            in_degree
                .entry(edge.to_node)
                .and_modify(|count| *count += 1);
            adj_list
                .entry(edge.from_node)
                .or_default()
                .push(edge.to_node);
        }

        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(&node_id, _)| node_id)
            .collect();

        let mut sorted = Vec::new();

        while let Some(node_id) = queue.pop_front() {
            sorted.push(node_id);

            if let Some(neighbors) = adj_list.get(&node_id) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(&neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        if sorted.len() != self.nodes.len() {
            return Err("Graph contains a cycle.");
        }

        Ok(sorted)
    }

    pub fn add_node<T: 'static + NodeEntity>(&mut self, node: T) -> NodeId {
        let node_id = Ulid::new();
        self.nodes.insert(node_id, Box::new(node));
        node_id
    }

    pub fn add_edge(&mut self, edge: Edge) -> EdgeId {
        let edge_id = Ulid::new();

        if let Some(input_slots) = self.context.inputs.get_mut(&edge.to_node) {
            if let Some(slot) = input_slots.iter_mut().find(|s| s.id == edge.to_slot) {
                slot.connected_edges.push(edge_id);
            }
        }

        if let Some(output_slots) = self.context.outputs.get_mut(&edge.from_node) {
            if let Some(slot) = output_slots.iter_mut().find(|s| s.id == edge.from_slot) {
                slot.connected_edges.push(edge_id);
            }
        }

        self.edges.insert(edge_id, edge);
        edge_id
    }

    pub fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), &'static str> {
        if let Some(edge) = self.edges.remove(&edge_id) {
            if let Some(input_slots) = self.context.inputs.get_mut(&edge.to_node) {
                if let Some(slot) = input_slots.iter_mut().find(|s| s.id == edge.to_slot) {
                    slot.connected_edges.retain(|&id| id != edge_id);
                }
            }

            if let Some(output_slots) = self.context.outputs.get_mut(&edge.from_node) {
                if let Some(slot) = output_slots.iter_mut().find(|s| s.id == edge.from_slot) {
                    slot.connected_edges.retain(|&id| id != edge_id);
                }
            }

            Ok(())
        } else {
            Err("Edge not found.")
        }
    }

    pub fn update_edge(&mut self, edge_id: EdgeId, new_edge: Edge) -> Result<(), &'static str> {
        if self.edges.contains_key(&edge_id) {
            self.edges.insert(edge_id, new_edge);
            Ok(())
        } else {
            Err("Edge not found.")
        }
    }

    // TODO: fix arg slot_id: Ulid
    pub fn get_edges_by_slot(&self, slot_id: Ulid) -> Vec<&Edge> {
        self.edges
            .values()
            .filter(|edge| edge.from_slot == slot_id || edge.to_slot == slot_id)
            .collect()
    }

    pub fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge> {
        self.edges.get(&edge_id)
    }

    pub fn debug_edges(&self) {
        for (edge_id, edge) in &self.edges {
            println!(
                "Edge {}: Node {} [Slot {}] -> Node {} [Slot {}]",
                edge_id, edge.from_node, edge.from_slot, edge.to_node, edge.to_slot
            );
        }
    }

    pub fn remove_node(&mut self, node_id: NodeId) -> Result<(), &'static str> {
        if self.nodes.remove(&node_id).is_some() {
            self.edges
                .retain(|_, edge| edge.from_node != node_id && edge.to_node != node_id);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    pub fn update_node(
        &mut self,
        node_id: NodeId,
        new_node: Box<dyn NodeEntity>,
    ) -> Result<(), &'static str> {
        if self.nodes.contains_key(&node_id) {
            self.nodes.insert(node_id, new_node);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    pub fn get_node_by_id(&self, node_id: NodeId) -> Option<&dyn NodeEntity> {
        self.nodes.get(&node_id).map(|node| &**node)
    }

    pub fn get_nodes_by_type<T: 'static + NodeEntity>(&self) -> Vec<(NodeId, &T)> {
        self.nodes
            .iter()
            .filter_map(|(&id, node)| {
                node.as_ref()
                    .as_any()
                    .downcast_ref::<T>()
                    .map(|typed_node| (id, typed_node))
            })
            .collect()
    }

    pub fn get_all_nodes(&self) -> Vec<(NodeId, &dyn NodeEntity)> {
        self.nodes.iter().map(|(&id, node)| (id, &**node)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestNode {
        id: u32,
    }

    impl NodeEntity for TestNode {
        fn execute(&self) {
            println!("Executing TestNode with id: {}", self.id);
        }
    }

    #[test]
    fn test_execute() {
        let mut node_graph = NodeGraph::new();

        let node1 = TestNode { id: 1 };
        let node2 = TestNode { id: 2 };
        let node3 = TestNode { id: 3 };

        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);
        let node3_id = node_graph.add_node(node3);

        node_graph.add_edge(Edge {
            from_node: node1_id,
            from_slot: Ulid::new(),
            to_node: node2_id,
            to_slot: Ulid::new(),
        });
        node_graph.add_edge(Edge {
            from_node: node2_id,
            from_slot: Ulid::new(),
            to_node: node3_id,
            to_slot: Ulid::new(),
        });

        assert!(node_graph.execute().is_ok());
    }

    #[test]
    fn test_topological_sort() {
        let mut node_graph = NodeGraph::new();

        let node1 = TestNode { id: 1 };
        let node2 = TestNode { id: 2 };
        let node3 = TestNode { id: 3 };

        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);
        let node3_id = node_graph.add_node(node3);

        node_graph.add_edge(Edge {
            from_node: node1_id,
            from_slot: Ulid::new(),
            to_node: node2_id,
            to_slot: Ulid::new(),
        });
        node_graph.add_edge(Edge {
            from_node: node2_id,
            from_slot: Ulid::new(),
            to_node: node3_id,
            to_slot: Ulid::new(),
        });

        let sorted = node_graph.topological_sort().unwrap();
        assert_eq!(sorted, vec![node1_id, node2_id, node3_id]);
    }

    #[test]
    fn test_cycle_detection() {
        let mut node_graph = NodeGraph::new();

        let node1 = TestNode { id: 1 };
        let node2 = TestNode { id: 2 };

        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);

        node_graph.add_edge(Edge {
            from_node: node1_id,
            from_slot: Ulid::new(),
            to_node: node2_id,
            to_slot: Ulid::new(),
        });
        node_graph.add_edge(Edge {
            from_node: node2_id,
            from_slot: Ulid::new(),
            to_node: node1_id,
            to_slot: Ulid::new(),
        });

        assert!(node_graph.topological_sort().is_err());
    }

    #[test]
    fn test_add_and_get_node() {
        let mut node_graph = NodeGraph::new();
        let node = TestNode { id: 1 };
        let node_id = node_graph.add_node(node);

        assert!(node_graph.get_node_by_id(node_id).is_some());
    }

    #[test]
    fn test_remove_node() {
        let mut node_graph = NodeGraph::new();
        let node = TestNode { id: 1 };
        let node_id = node_graph.add_node(node);

        assert!(node_graph.remove_node(node_id).is_ok());
        assert!(node_graph.get_node_by_id(node_id).is_none());
    }

    #[test]
    fn test_add_and_get_edge() {
        let mut node_graph = NodeGraph::new();

        let node1 = TestNode { id: 1 };
        let node2 = TestNode { id: 2 };
        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);

        let edge = Edge {
            from_node: node1_id,
            from_slot: Ulid::new(),
            to_node: node2_id,
            to_slot: Ulid::new(),
        };
        let edge_id = node_graph.add_edge(edge);

        assert!(node_graph.get_edge(edge_id).is_some());
    }

    #[test]
    fn test_remove_edge() {
        let mut node_graph = NodeGraph::new();

        let node1 = TestNode { id: 1 };
        let node2 = TestNode { id: 2 };
        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);

        let edge = Edge {
            from_node: node1_id,
            from_slot: Ulid::new(),
            to_node: node2_id,
            to_slot: Ulid::new(),
        };
        let edge_id = node_graph.add_edge(edge);

        assert!(node_graph.remove_edge(edge_id).is_ok());
        assert!(!node_graph.edges.contains_key(&edge_id));
    }
}
