use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
    fmt::Debug,
};

pub mod edge;
pub use edge::*;
use ulid::Ulid;

pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

macro_rules! impl_node_core {
    ($struct_name:ident) => {
        impl NodeCore for $struct_name {
            fn input_value<'a>(
                &'a self,
                evaluation_context: &'a EvaluationContext,
                slot_index: usize,
            ) -> Result<Vec<&'a Data>, String> {
                let mut result = Vec::new();
                let input_slot = match self.inputs().get(slot_index) {
                    Some(slot) => slot,
                    None => return Err("Invalid slot index".to_string()),
                };

                for edge_id in &input_slot.connected_edges {
                    if let Some(edge) = evaluation_context.edges.get(edge_id) {
                        if let Some(value) =
                            evaluation_context.outputs.get(&edge.from_output_slot_id)
                        {
                            result.push(value);
                        }
                    }
                }

                Ok(result)
            }

            fn set_output_value(
                &self,
                evaluation_context: &mut EvaluationContext,
                slot_index: usize,
                value: Data,
            ) -> Result<(), String> {
                match self.outputs().get(slot_index) {
                    Some(slot) => match (&slot.data_type, &value) {
                        (DataType::Number, Data::Number(number)) => {
                            evaluation_context
                                .outputs
                                .insert(slot.id.clone(), Data::Number(*number));
                            Ok(())
                        }
                        (DataType::String, Data::String(string)) => {
                            evaluation_context
                                .outputs
                                .insert(slot.id.clone(), Data::String(string.clone()));
                            Ok(())
                        }
                        (expected, actual) => Err(format!(
                            "Type mismatch: expected {:?}, but got {:?}",
                            expected, actual
                        )),
                    },
                    None => Err("Invalid value index".to_string()),
                }
            }

            fn get_input_slot_by_index(&self, input_slot_index: usize) -> Option<&InputSlot> {
                self.inputs().get(input_slot_index)
            }

            fn get_output_slot_by_index(&self, output_slot_index: usize) -> Option<&OutputSlot> {
                self.outputs().get(output_slot_index)
            }

            fn get_input_slot_by_index_mut(
                &mut self,
                input_slot_index: usize,
            ) -> Option<&mut InputSlot> {
                self.inputs.get_mut(input_slot_index)
            }

            fn get_output_slot_by_index_mut(
                &mut self,
                output_slot_index: usize,
            ) -> Option<&mut OutputSlot> {
                self.outputs.get_mut(output_slot_index)
            }

            fn inputs(&self) -> &Vec<InputSlot> {
                &self.inputs
            }

            fn inputs_mut(&mut self) -> &mut Vec<InputSlot> {
                &mut self.inputs
            }

            fn outputs(&self) -> &Vec<OutputSlot> {
                &self.outputs
            }

            fn outputs_mut(&mut self) -> &mut Vec<OutputSlot> {
                &mut self.outputs
            }
        }
    };
}

macro_rules! impl_primitive_node_core {
    ($struct_name:ident) => {
        impl NodePrimitive for $struct_name {}

        impl_node_core!($struct_name);
    };
}

pub trait NodeImpl: Debug + AsAny + Send + Sync + NodeCore {
    fn initialize() -> Self
    where
        Self: Sized;

    fn execute(&self, evaluation_context: &mut EvaluationContext) -> Result<(), String>;
}

pub trait NodePrimitive: NodeCore {
    fn set_default_value(
        &self,
        evaluation_context: &mut EvaluationContext,
        value: Data,
    ) -> Result<(), String> {
        self.set_output_value(evaluation_context, 0, value)
    }
}

pub trait NodeCore {
    fn input_value<'a>(
        &'a self,
        evaluation_context: &'a EvaluationContext,
        slot_index: usize,
    ) -> Result<Vec<&'a Data>, String>;

    fn get_input_slot_by_index(&self, input_slot_index: usize) -> Option<&InputSlot>;

    fn get_output_slot_by_index(&self, output_slot_index: usize) -> Option<&OutputSlot>;

    fn get_input_slot_by_index_mut(&mut self, input_slot_index: usize) -> Option<&mut InputSlot>;

    fn set_output_value(
        &self,
        evaluation_context: &mut EvaluationContext,
        slot_index: usize,
        value: Data,
    ) -> Result<(), String>;

    fn get_output_slot_by_index_mut(&mut self, output_slot_index: usize)
        -> Option<&mut OutputSlot>;

    fn inputs(&self) -> &Vec<InputSlot>;

    fn inputs_mut(&mut self) -> &mut Vec<InputSlot>;

    fn outputs(&self) -> &Vec<OutputSlot>;

    fn outputs_mut(&mut self) -> &mut Vec<OutputSlot>;
}

impl dyn NodeImpl {
    pub fn downcast_ref<T: NodeImpl + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    pub fn downcast_mut<T: NodeImpl + 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

impl<T: 'static + NodeImpl> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub type NodeId = Ulid;
pub type EdgeId = Ulid;

#[derive(Debug, PartialEq)]
pub enum Data {
    Number(f32),
    String(String),
}

#[derive(Default, Debug)]
pub struct EvaluationContext {
    edges: HashMap<EdgeId, Edge>,
    outputs: HashMap<OutputSlotId, Data>,
}

#[derive(Default, Debug)]
pub struct NodeGraph {
    pub nodes: HashMap<NodeId, Box<dyn NodeImpl>>,
    pub context: EvaluationContext,
    pub affected_nodes: HashSet<NodeId>,
}

impl NodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn execute(&mut self) -> Result<(), String> {
        if self.affected_nodes.is_empty() {
            return Ok(());
        }

        let sorted_nodes = self.topological_sort(&self.affected_nodes)?;
        for node_id in sorted_nodes {
            if let Some(node) = self.nodes.get(&node_id) {
                node.execute(&mut self.context)?;
            }
        }

        self.affected_nodes.clear();
        Ok(())
    }

    fn create_edge(
        &self,
        from_node_id: NodeId,
        from_output_slot_index: usize,
        to_node_id: NodeId,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str> {
        let from_output_slot_id = {
            let node = self
                .get_node_by_id(from_node_id)
                .ok_or("From node not found")?;
            node.outputs()
                .get(from_output_slot_index)
                .ok_or("Invalid output slot index")?
                .id
                .clone()
        };

        let to_input_slot_id = {
            let node = self.get_node_by_id(to_node_id).ok_or("To node not found")?;
            node.inputs()
                .get(to_input_slot_index)
                .ok_or("Invalid input slot index")?
                .id
                .clone()
        };
        Ok(Edge {
            from_node_id,
            from_output_slot_index,
            from_output_slot_id,
            to_node_id,
            to_input_slot_index,
            to_input_slot_id,
        })
    }

    fn mark_affected_nodes(&mut self, nodes: Vec<NodeId>) {
        self.affected_nodes.extend(nodes.into_iter());
    }

    pub fn topological_sort(
        &self,
        target_nodes: &HashSet<NodeId>,
    ) -> Result<Vec<NodeId>, &'static str> {
        let mut in_degree = HashMap::new();
        let mut adj_list = HashMap::new();

        for &node_id in target_nodes {
            in_degree.insert(node_id, 0);
            adj_list.insert(node_id, Vec::new());
        }
        for edge in self.context.edges.values() {
            if target_nodes.contains(&edge.from_node_id) && target_nodes.contains(&edge.to_node_id)
            {
                in_degree
                    .entry(edge.to_node_id)
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
                adj_list
                    .entry(edge.from_node_id)
                    .or_default()
                    .push(edge.to_node_id);
            }
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

        if sorted.len() != target_nodes.len() {
            return Err("Graph contains a cycle.");
        }

        Ok(sorted)
    }

    pub fn add_node<T: 'static + NodeImpl>(&mut self, node: T) -> NodeId {
        let node_id = Ulid::new();
        let node_box = Box::new(node);
        self.nodes.insert(node_id, node_box);
        self.mark_affected_nodes(vec![node_id]);
        node_id
    }

    pub fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String> {
        let from_node_id = edge.from_node_id;
        let to_node_id = edge.to_node_id;

        let from_node = self
            .nodes
            .get(&from_node_id)
            .ok_or_else(|| format!("From node {:?} does not exist", edge.from_node_id))?;
        let to_node = self
            .nodes
            .get(&to_node_id)
            .ok_or_else(|| format!("To node {:?} does not exist", edge.to_node_id))?;

        let from_slot = from_node
            .get_output_slot_by_index(edge.from_output_slot_index)
            .ok_or_else(|| {
                format!(
                    "Output slot index {:?} does not exist in node {:?}",
                    edge.from_output_slot_index, edge.from_node_id
                )
            })?;
        let to_slot = to_node
            .get_input_slot_by_index(edge.to_input_slot_index)
            .ok_or_else(|| {
                format!(
                    "Input slot index {:?} does not exist in node {:?}",
                    edge.to_input_slot_index, edge.to_node_id
                )
            })?;

        if from_slot.data_type != to_slot.data_type {
            return Err(format!(
                "Data type mismatch between output slot index {:?} and input slot index {:?}",
                edge.from_output_slot_index, edge.to_input_slot_index
            ));
        }

        let edge_id = EdgeId::new();

        if let Some(node) = self.nodes.get_mut(&from_node_id) {
            if let Some(slot) = node.get_output_slot_by_index_mut(edge.from_output_slot_index) {
                slot.connected_edges.push(edge_id);
            }
        }

        if let Some(node) = self.nodes.get_mut(&to_node_id) {
            if let Some(slot) = node.get_input_slot_by_index_mut(edge.to_input_slot_index) {
                slot.connected_edges.push(edge_id);
            }
        }

        let affected_nodes = self.collect_affected_nodes(vec![from_node_id, to_node_id]);
        self.mark_affected_nodes(affected_nodes);
        self.context.edges.insert(edge_id, edge);

        Ok(edge_id)
    }

    pub fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), &'static str> {
        let edge = self
            .context
            .edges
            .remove(&edge_id)
            .ok_or("Edge does not exist")?;

        let from_node_id = edge.from_node_id;
        let to_node_id = edge.to_node_id;

        if let Some(node) = self.nodes.get_mut(&from_node_id) {
            if let Some(slot) = node.get_output_slot_by_index_mut(edge.from_output_slot_index) {
                slot.connected_edges.retain(|&id| id != edge_id);
            }
        }

        if let Some(node) = self.nodes.get_mut(&to_node_id) {
            if let Some(slot) = node.get_input_slot_by_index_mut(edge.to_input_slot_index) {
                slot.connected_edges.retain(|&id| id != edge_id);
            }
        }

        let affected_nodes = self.collect_affected_nodes(vec![from_node_id, to_node_id]);
        self.mark_affected_nodes(affected_nodes);

        Ok(())
    }

    pub fn update_edge(&mut self, edge_id: EdgeId, new_edge: Edge) -> Result<(), &'static str> {
        let old_edge = self
            .context
            .edges
            .get(&edge_id)
            .ok_or("Edge does not exist")?;

        let old_from_node_id = old_edge.from_node_id;
        let old_to_node_id = old_edge.to_node_id;

        let new_from_node = self
            .nodes
            .get(&new_edge.from_node_id)
            .ok_or("New from_node does not exist")?;
        let new_to_node = self
            .nodes
            .get(&new_edge.to_node_id)
            .ok_or("New to_node does not exist")?;

        let new_from_slot = new_from_node
            .get_output_slot_by_index(new_edge.from_output_slot_index)
            .ok_or("New output slot does not exist")?;
        let new_to_slot = new_to_node
            .get_input_slot_by_index(new_edge.to_input_slot_index)
            .ok_or("New input slot does not exist")?;

        if new_from_slot.data_type != new_to_slot.data_type {
            return Err("Data type mismatch between new output and input slots");
        }

        if let Some(node) = self.nodes.get_mut(&old_from_node_id) {
            if let Some(slot) = node.get_output_slot_by_index_mut(old_edge.from_output_slot_index) {
                slot.connected_edges.retain(|&id| id != edge_id);
            }
        }

        if let Some(node) = self.nodes.get_mut(&old_to_node_id) {
            if let Some(slot) = node.get_input_slot_by_index_mut(old_edge.to_input_slot_index) {
                slot.connected_edges.retain(|&id| id != edge_id);
            }
        }

        if let Some(node) = self.nodes.get_mut(&new_edge.from_node_id) {
            if let Some(slot) = node.get_output_slot_by_index_mut(new_edge.from_output_slot_index) {
                slot.connected_edges.push(edge_id);
            }
        }

        if let Some(node) = self.nodes.get_mut(&new_edge.to_node_id) {
            if let Some(slot) = node.get_input_slot_by_index_mut(new_edge.to_input_slot_index) {
                slot.connected_edges.push(edge_id);
            }
        }

        let affected_nodes = self.collect_affected_nodes(vec![
            old_from_node_id,
            old_to_node_id,
            new_edge.from_node_id,
            new_edge.to_node_id,
        ]);
        self.mark_affected_nodes(affected_nodes);

        self.context.edges.insert(edge_id, new_edge);

        Ok(())
    }

    pub fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge> {
        self.context.edges.get(&edge_id)
    }

    pub fn remove_node(&mut self, node_id: NodeId) -> Result<(), &'static str> {
        if self.nodes.remove(&node_id).is_some() {
            let affected_nodes = self.collect_affected_nodes(vec![node_id]);
            self.context
                .edges
                .retain(|_, edge| edge.from_node_id != node_id && edge.to_node_id != node_id);
            self.mark_affected_nodes(affected_nodes);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    pub fn update_node<T: 'static + NodeImpl>(
        &mut self,
        node_id: NodeId,
        new_node: T,
    ) -> Result<(), &'static str> {
        if self.nodes.contains_key(&node_id) {
            let node_box = Box::new(new_node);
            self.nodes.insert(node_id, node_box);
            let affected_nodes = self.collect_affected_nodes(vec![node_id]);
            self.mark_affected_nodes(affected_nodes);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }
    pub fn get_node_by_id(&self, node_id: NodeId) -> Option<&Box<dyn NodeImpl>> {
        self.nodes.get(&node_id).map(|node| node)
    }

    pub fn get_nodes_by_type<T: NodeImpl + 'static>(&self) -> Vec<(NodeId, &T)> {
        self.nodes
            .iter()
            .filter_map(|(&id, node)| node.downcast_ref::<T>().map(|typed_node| (id, typed_node)))
            .collect()
    }

    pub fn get_all_nodes(&self) -> Vec<(NodeId, &Box<dyn NodeImpl>)> {
        self.nodes.iter().map(|(&id, node)| (id, node)).collect()
    }

    fn collect_affected_nodes(&self, initial_nodes: Vec<NodeId>) -> Vec<NodeId> {
        let mut affected = HashSet::new();
        let mut queue = VecDeque::from(initial_nodes);

        while let Some(node_id) = queue.pop_front() {
            if affected.insert(node_id) {
                for edge in self.context.edges.values() {
                    if edge.from_node_id == node_id && !affected.contains(&edge.to_node_id) {
                        queue.push_back(edge.to_node_id);
                    }
                }
            }
        }

        affected.into_iter().collect()
    }

    fn get_output_value(&self, node_id: NodeId, output_slot_index: usize) -> Option<&Data> {
        let node = self.get_node_by_id(node_id)?;
        let slot = node.outputs().get(output_slot_index)?;
        self.context.outputs.get(&slot.id)
    }
}

#[derive(Debug)]
struct AddNode {
    node_name: &'static str,
    inputs: Vec<InputSlot>,
    outputs: Vec<OutputSlot>,
}

impl_node_core!(AddNode);

impl NodeImpl for AddNode {
    fn initialize() -> Self {
        Self {
            node_name: "Addition",
            inputs: vec![InputSlot {
                label: "Number List",
                data_type: DataType::Number,
                ..Default::default()
            }],
            outputs: vec![OutputSlot {
                label: "Result",
                data_type: DataType::Number,
                ..Default::default()
            }],
        }
    }
    fn execute(&self, evaluation_context: &mut EvaluationContext) -> Result<(), String> {
        let data = self.input_value(evaluation_context, 0)?;
        let mut result = 0.;
        for d in data {
            match d {
                Data::Number(value) => result += value,
                _ => return Err("Expected number".to_string()),
            }
        }
        self.set_output_value(evaluation_context, 0, Data::Number(result))?;
        Ok(())
    }
}

#[derive(Debug)]
struct NumberNode {
    node_name: &'static str,
    inputs: Vec<InputSlot>,
    outputs: Vec<OutputSlot>,
}

impl_primitive_node_core!(NumberNode);

impl NodeImpl for NumberNode {
    fn initialize() -> Self {
        Self {
            node_name: "Number",
            inputs: vec![],
            outputs: vec![OutputSlot {
                label: "Value",
                data_type: DataType::Number,
                ..Default::default()
            }],
        }
    }

    fn execute(&self, _evaluation_context: &mut EvaluationContext) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
        let node3 = AddNode::initialize();

        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);
        let node3_id = node_graph.add_node(node3);

        let edge_1_3 = node_graph.create_edge(node1_id, 0, node3_id, 0).unwrap();
        let edge_2_3 = node_graph.create_edge(node2_id, 0, node3_id, 0).unwrap();

        node_graph.add_edge(edge_1_3).unwrap();
        node_graph.add_edge(edge_2_3).unwrap();

        assert!(node_graph.execute().is_ok());
        let res = node_graph.get_output_value(node3_id, 0).unwrap();
        assert_eq!(res, &Data::Number(30.));
    }

    #[test]
    fn test_add_and_get_node() {
        let mut node_graph = NodeGraph::new();
        let node = AddNode::initialize();
        let node_id = node_graph.add_node(node);

        assert!(node_graph.get_node_by_id(node_id).is_some());
    }

    #[test]
    fn test_remove_node() {
        let mut node_graph = NodeGraph::new();
        let node = AddNode::initialize();
        let node_id = node_graph.add_node(node);

        assert!(node_graph.remove_node(node_id).is_ok());
        assert!(node_graph.get_node_by_id(node_id).is_none());
    }

    #[test]
    fn test_add_and_get_edge() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddNode::initialize();
        let node2 = AddNode::initialize();
        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);

        let edge_1_2 = node_graph.create_edge(node1_id, 0, node2_id, 0).unwrap();

        let edge_id = node_graph.add_edge(edge_1_2).unwrap();

        assert!(node_graph.get_edge(edge_id).is_some());
    }

    #[test]
    fn test_remove_edge() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddNode::initialize();
        let node2 = AddNode::initialize();
        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);

        let edge_1_2 = node_graph.create_edge(node1_id, 0, node2_id, 0).unwrap();

        let edge_id = node_graph.add_edge(edge_1_2).unwrap();

        assert!(node_graph.remove_edge(edge_id).is_ok());
        assert!(!node_graph.context.edges.contains_key(&edge_id));
    }

    #[test]
    fn test_node_execution() {
        let mut node_graph = NodeGraph::new();

        let node1 = AddNode::initialize();
        let node2 = AddNode::initialize();
        let node3 = AddNode::initialize();

        let node1_id = node_graph.add_node(node1);
        let node2_id = node_graph.add_node(node2);
        let node3_id = node_graph.add_node(node3);

        let edge_1_2 = node_graph.create_edge(node1_id, 0, node2_id, 0).unwrap();
        let edge_2_3 = node_graph.create_edge(node2_id, 0, node3_id, 0).unwrap();

        node_graph.add_edge(edge_1_2).unwrap();
        node_graph.add_edge(edge_2_3).unwrap();

        node_graph.execute().unwrap();

        println!("--- first execute finished---");

        node_graph
            .update_node(node2_id, AddNode::initialize())
            .unwrap();

        node_graph.execute().unwrap();
    }
}
