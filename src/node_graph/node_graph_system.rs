use crate::{Edge, EdgeId, EvaluationContext, NodeGraph, NodeGraphAPI, NodeId, NodeManager};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) trait NodeGraphSystem {
    fn topological_sort(&self, target_nodes: &HashSet<NodeId>)
        -> Result<Vec<NodeId>, &'static str>;

    fn create_edge(
        &self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str>;

    fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String>;

    fn collect_dirty_nodes(&self, initial_nodes: Vec<NodeId>) -> Vec<NodeId>;

    fn mark_dirty_nodes(&mut self, nodes: Vec<NodeId>);

    fn resources_mut(&mut self) -> (&mut NodeManager, &mut EvaluationContext);
}

impl NodeGraphSystem for NodeGraph {
    fn topological_sort(
        &self,
        target_nodes: &HashSet<NodeId>,
    ) -> Result<Vec<NodeId>, &'static str> {
        let mut in_degree = HashMap::new();
        let mut adj_list = HashMap::new();

        for node_id in target_nodes {
            in_degree.insert(node_id.clone(), 0);
            adj_list.insert(node_id.clone(), Vec::new());
        }
        for edge in self.context.edges.values() {
            if target_nodes.contains(&edge.from_node_id) && target_nodes.contains(&edge.to_node_id)
            {
                in_degree
                    .entry(edge.to_node_id.clone())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
                adj_list
                    .entry(edge.from_node_id.clone())
                    .or_default()
                    .push(edge.to_node_id.clone());
            }
        }

        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(node_id, _)| node_id.clone())
            .collect();

        let mut sorted = Vec::new();

        while let Some(node_id) = queue.pop_front() {
            sorted.push(node_id.clone());

            if let Some(neighbors) = adj_list.get(&node_id) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(&neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor.clone());
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

    fn create_edge(
        &self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
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
            from_node_id: from_node_id.clone(),
            from_output_slot_index,
            from_output_slot_id,
            to_node_id: to_node_id.clone(),
            to_input_slot_index,
            to_input_slot_id,
        })
    }

    fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String> {
        let from_node_id = edge.from_node_id.clone();
        let to_node_id = edge.to_node_id.clone();

        let from_node = self
            .node_manager
            .nodes()
            .get(&from_node_id)
            .ok_or_else(|| format!("From node {:?} does not exist", edge.from_node_id))?;
        let to_node = self
            .node_manager
            .nodes()
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

        if let Some(node) = self.node_manager.nodes_mut().get_mut(&from_node_id) {
            if let Some(slot) = node.get_output_slot_by_index_mut(edge.from_output_slot_index) {
                slot.connected_edges.push(edge_id.clone());
            }
        }

        if let Some(node) = self.node_manager.nodes_mut().get_mut(&to_node_id) {
            if let Some(slot) = node.get_input_slot_by_index_mut(edge.to_input_slot_index) {
                slot.connected_edges.push(edge_id.clone());
            }
        }

        let dirty_nodes = self.collect_dirty_nodes(vec![from_node_id, to_node_id]);
        self.mark_dirty_nodes(dirty_nodes);
        self.context.edges.insert(edge_id.clone(), edge);

        Ok(edge_id)
    }

    fn collect_dirty_nodes(&self, initial_nodes: Vec<NodeId>) -> Vec<NodeId> {
        let mut affected = HashSet::new();
        let mut queue = VecDeque::from(initial_nodes);

        while let Some(node_id) = queue.pop_front() {
            if affected.insert(node_id.clone()) {
                for edge in self.context.edges.values() {
                    if edge.from_node_id == node_id && !affected.contains(&edge.to_node_id) {
                        queue.push_back(edge.to_node_id.clone());
                    }
                }
            }
        }

        affected.into_iter().collect()
    }

    fn mark_dirty_nodes(&mut self, nodes: Vec<NodeId>) {
        self.dirty_nodes.extend(nodes.into_iter());
    }

    fn resources_mut(&mut self) -> (&mut NodeManager, &mut EvaluationContext) {
        (&mut self.node_manager, &mut self.context)
    }
}
