use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
    fmt::Debug,
    sync::Arc,
};

pub mod edge;
pub use edge::*;
use ulid::Ulid;

pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub trait NodeEntity: Debug + AsAny + Send + Sync {
    fn execute(&self, evaluation_context: &mut EvaluationContext);
}

impl dyn NodeEntity {
    pub fn downcast_ref<T: NodeEntity + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    pub fn downcast_mut<T: NodeEntity + 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
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
    pub nodes: HashMap<NodeId, Arc<dyn NodeEntity>>,
    pub edges: HashMap<EdgeId, Edge>,
    pub context: EvaluationContext,
    pub affected_nodes: HashSet<NodeId>,
}

impl NodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn execute(&mut self) -> Result<(), &'static str> {
        if self.affected_nodes.is_empty() {
            return Ok(());
        }

        let sorted_nodes = self.topological_sort()?;
        for node_id in sorted_nodes {
            if self.affected_nodes.contains(&node_id) {
                if let Some(node) = self.nodes.get(&node_id) {
                    node.execute(&mut self.context);
                }
            }
        }

        self.affected_nodes.clear();
        Ok(())
    }

    fn mark_affected_nodes(&mut self, nodes: Vec<NodeId>) {
        self.affected_nodes.extend(nodes.into_iter());
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
        let node_arc = Arc::new(node);
        self.nodes.insert(node_id, node_arc);
        self.mark_affected_nodes(vec![node_id]);
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

        let from_node = edge.from_node;
        let to_node = edge.to_node;
        self.edges.insert(edge_id, edge);

        let affected_nodes = self.collect_affected_nodes(vec![from_node, to_node]);
        self.mark_affected_nodes(affected_nodes);

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

            let affected_nodes = self.collect_affected_nodes(vec![edge.from_node, edge.to_node]);
            self.mark_affected_nodes(affected_nodes);

            Ok(())
        } else {
            Err("Edge not found.")
        }
    }

    pub fn update_edge(&mut self, edge_id: EdgeId, new_edge: Edge) -> Result<(), &'static str> {
        if let Some(old_edge) = self.edges.get(&edge_id) {
            if let Some(input_slots) = self.context.inputs.get_mut(&old_edge.to_node) {
                if let Some(slot) = input_slots.iter_mut().find(|s| s.id == old_edge.to_slot) {
                    slot.connected_edges.retain(|&id| id != edge_id);
                }
            }

            if let Some(output_slots) = self.context.outputs.get_mut(&old_edge.from_node) {
                if let Some(slot) = output_slots.iter_mut().find(|s| s.id == old_edge.from_slot) {
                    slot.connected_edges.retain(|&id| id != edge_id);
                }
            }
        }

        if let Some(input_slots) = self.context.inputs.get_mut(&new_edge.to_node) {
            if let Some(slot) = input_slots.iter_mut().find(|s| s.id == new_edge.to_slot) {
                slot.connected_edges.push(edge_id);
            }
        }

        if let Some(output_slots) = self.context.outputs.get_mut(&new_edge.from_node) {
            if let Some(slot) = output_slots.iter_mut().find(|s| s.id == new_edge.from_slot) {
                slot.connected_edges.push(edge_id);
            }
        }

        let from_node = new_edge.from_node;
        let to_node = new_edge.to_node;
        self.edges.insert(edge_id, new_edge);

        let affected_nodes = self.collect_affected_nodes(vec![from_node, to_node]);
        self.mark_affected_nodes(affected_nodes);

        Ok(())
    }

    pub fn get_edges_by_slot(&self, slot_id: SlotId) -> Vec<&Edge> {
        self.edges
            .values()
            .filter(|edge| match &slot_id {
                SlotId::Input(id) => edge.to_slot == *id,
                SlotId::Output(id) => edge.from_slot == *id,
            })
            .collect()
    }

    pub fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge> {
        self.edges.get(&edge_id)
    }

    pub fn remove_node(&mut self, node_id: NodeId) -> Result<(), &'static str> {
        if self.nodes.remove(&node_id).is_some() {
            let affected_nodes = self.collect_affected_nodes(vec![node_id]);
            self.edges
                .retain(|_, edge| edge.from_node != node_id && edge.to_node != node_id);
            self.mark_affected_nodes(affected_nodes);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    pub fn update_node<T: 'static + NodeEntity>(
        &mut self,
        node_id: NodeId,
        new_node: T,
    ) -> Result<(), &'static str> {
        if self.nodes.contains_key(&node_id) {
            let node_arc = Arc::new(new_node);
            self.nodes.insert(node_id, node_arc);
            let affected_nodes = self.collect_affected_nodes(vec![node_id]);
            self.mark_affected_nodes(affected_nodes);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    pub fn get_node_by_id(&self, node_id: NodeId) -> Option<Arc<dyn NodeEntity>> {
        self.nodes.get(&node_id).map(|node| Arc::clone(node))
    }

    pub fn get_nodes_by_type<T: NodeEntity + 'static>(&self) -> Vec<(NodeId, &T)> {
        self.nodes
            .iter()
            .filter_map(|(&id, node)| node.downcast_ref::<T>().map(|typed_node| (id, typed_node)))
            .collect()
    }

    pub fn get_all_nodes(&self) -> Vec<(NodeId, Arc<dyn NodeEntity>)> {
        self.nodes
            .iter()
            .map(|(&id, node)| (id, Arc::clone(node)))
            .collect()
    }

    fn collect_affected_nodes(&self, initial_nodes: Vec<NodeId>) -> Vec<NodeId> {
        let mut affected = HashSet::new();
        let mut queue = VecDeque::from(initial_nodes);

        while let Some(node_id) = queue.pop_front() {
            if affected.insert(node_id) {
                for edge in self.edges.values() {
                    if edge.from_node == node_id && !affected.contains(&edge.to_node) {
                        queue.push_back(edge.to_node);
                    }
                }
            }
        }

        affected.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct AddNode {
        id: u32,
        executed_count: u8,
    }

    impl NodeEntity for AddNode {
        fn execute(&self, evaluation_context: &mut EvaluationContext) {
            // self.executed_count += 1;
            println!("Executing AddNode with id: {}", self.id);
        }
    }

    #[test]
    fn test_execute() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node2 = AddNode {
            id: 2,
            executed_count: 0,
        };
        let node3 = AddNode {
            id: 3,
            executed_count: 0,
        };

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

        let node1 = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node2 = AddNode {
            id: 2,
            executed_count: 0,
        };
        let node3 = AddNode {
            id: 3,
            executed_count: 0,
        };

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

        let node1 = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node2 = AddNode {
            id: 2,
            executed_count: 0,
        };

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
        let node = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node_id = node_graph.add_node(node);

        assert!(node_graph.get_node_by_id(node_id).is_some());
    }

    #[test]
    fn test_remove_node() {
        let mut node_graph = NodeGraph::new();
        let node = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node_id = node_graph.add_node(node);

        assert!(node_graph.remove_node(node_id).is_ok());
        assert!(node_graph.get_node_by_id(node_id).is_none());
    }

    #[test]
    fn test_add_and_get_edge() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node2 = AddNode {
            id: 2,
            executed_count: 0,
        };
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

        let node1 = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node2 = AddNode {
            id: 2,
            executed_count: 0,
        };
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

    #[test]
    fn test_node_execution() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddNode {
            id: 1,
            executed_count: 0,
        };
        let node2 = AddNode {
            id: 2,
            executed_count: 0,
        };
        let node3 = AddNode {
            id: 3,
            executed_count: 0,
        };

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

        node_graph.execute().unwrap();

        println!("--- first execute finished---");

        let add_node_ref = node_graph.get_node_by_id(node2_id).unwrap();
        let node = add_node_ref.downcast_ref::<AddNode>().unwrap().clone();
        node_graph.update_node(node2_id, node).unwrap();

        node_graph.execute().unwrap();

        let executed_1 = node_graph
            .get_node_by_id(node1_id)
            .and_then(|node| node.downcast_ref::<AddNode>().cloned())
            .map_or(false, |n| n.executed_count == 1);

        let executed_2 = node_graph
            .get_node_by_id(node2_id)
            .and_then(|node| node.downcast_ref::<AddNode>().cloned())
            .map_or(false, |n| n.executed_count == 2);

        let executed_3 = node_graph
            .get_node_by_id(node3_id)
            .and_then(|node| node.downcast_ref::<AddNode>().cloned())
            .map_or(false, |n| n.executed_count == 2);

        assert!(executed_1);
        assert!(executed_2);

        assert!(executed_3);
    }
}
