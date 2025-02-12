use crate::{Edge, EdgeId, EntityId, NodeGraph, NodeGraphAPI, NodeId};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) trait NodeGraphSystem {
    fn topological_sort(&self, target_nodes: &HashSet<NodeId>) -> Result<Vec<Vec<NodeId>>, String>;

    async fn create_edge(
        &self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str>;

    async fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String>;

    fn collect_dirty_nodes(&self, initial_nodes: Vec<NodeId>) -> Result<Vec<NodeId>, String>;

    fn mark_dirty_nodes(&mut self, nodes: Vec<NodeId>);

    fn remove_edges_from_cache(&self, node_id: &NodeId) -> Result<(), String>;

    fn remove_edge_from_cache(&self, edge_id: &EdgeId) -> Result<Edge, String>;
}

impl NodeGraphSystem for NodeGraph {
    fn topological_sort(&self, target_nodes: &HashSet<NodeId>) -> Result<Vec<Vec<NodeId>>, String> {
        let mut in_degree = HashMap::new();
        let mut adj_list = HashMap::new();

        for node_id in target_nodes {
            in_degree.insert(node_id, 0);
            adj_list.insert(node_id, Vec::new());
        }

        let cache = self.cache.lock()?;
        for edge in cache.edges.values() {
            if target_nodes.contains(&edge.from_node_id) && target_nodes.contains(&edge.to_node_id)
            {
                in_degree
                    .entry(&edge.to_node_id)
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
                adj_list
                    .entry(&edge.from_node_id)
                    .or_default()
                    .push(edge.to_node_id);
            }
        }

        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(node_id, _)| **node_id)
            .collect();

        let mut sorted = Vec::new();

        while !queue.is_empty() {
            let mut current_level = Vec::new();

            for _ in 0..queue.len() {
                if let Some(node_id) = queue.pop_front() {
                    current_level.push(node_id);

                    if let Some(neighbors) = adj_list.get(&node_id) {
                        for neighbor in neighbors {
                            if let Some(deg) = in_degree.get_mut(neighbor) {
                                *deg -= 1;
                                if *deg == 0 {
                                    queue.push_back(*neighbor);
                                }
                            }
                        }
                    }
                }
            }

            sorted.push(current_level);
        }

        let total_count: usize = sorted.iter().map(|level| level.len()).sum();
        if total_count != target_nodes.len() {
            return Err("Graph contains a cycle.".to_string());
        }

        Ok(sorted)
    }

    async fn create_edge(
        &self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str> {
        let from_output_slot_id = {
            let node = self
                .get_node_by_id(from_node_id)
                .await
                .ok_or("From node not found")?;
            let read_node = node.read().await;
            read_node
                .outputs()
                .get(from_output_slot_index)
                .ok_or("Invalid output slot index")?
                .id
                .clone()
        };

        let to_input_slot_id = {
            let node = self
                .get_node_by_id(to_node_id)
                .await
                .ok_or("To node not found")?;
            let read_node = node.read().await;
            read_node
                .inputs()
                .get(to_input_slot_index)
                .ok_or("Invalid input slot index")?
                .id
                .clone()
        };

        Ok(Edge {
            from_node_id: *from_node_id,
            from_output_slot_index,
            from_output_slot_id,
            to_node_id: *to_node_id,
            to_input_slot_index,
            to_input_slot_id,
        })
    }

    async fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String> {
        let from_node_id = edge.from_node_id;
        let to_node_id = edge.to_node_id;

        let from_node = self
            .node_manager
            .get_node_by_id(&from_node_id)
            .await
            .ok_or_else(|| format!("From node {:?} does not exist", edge.from_node_id))?;
        let to_node = self
            .node_manager
            .get_node_by_id(&to_node_id)
            .await
            .ok_or_else(|| format!("To node {:?} does not exist", edge.to_node_id))?;

        let mut write_from_node = from_node.write().await;
        let from_slot = write_from_node
            .get_output_slot_by_index(edge.from_output_slot_index)
            .ok_or_else(|| {
                format!(
                    "Output slot index {:?} does not exist in node {:?}",
                    edge.from_output_slot_index, edge.from_node_id
                )
            })?;

        let mut write_to_node = to_node.write().await;
        let to_slot = write_to_node
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

        if let Some(slot) =
            write_from_node.get_output_slot_by_index_mut(edge.from_output_slot_index)
        {
            slot.connected_edges.push(edge_id);
        }

        if let Some(slot) = write_to_node.get_input_slot_by_index_mut(edge.to_input_slot_index) {
            if let Some(max) = slot.max_connections() {
                if slot.connected_edges.len() >= *max {
                    return Err(format!(
                        "Input slot index {:?} in node: {} reached maximum connection limit ({})",
                        edge.to_input_slot_index,
                        edge.to_node_id.id_string(),
                        max
                    ));
                }
            }
            slot.connected_edges.push(edge_id);
        }

        let dirty_nodes = self.collect_dirty_nodes(vec![from_node_id, to_node_id])?;
        self.mark_dirty_nodes(dirty_nodes);
        let mut cache = self.cache.lock()?;
        cache.edges.insert(edge_id, edge);

        Ok(edge_id)
    }

    fn collect_dirty_nodes(&self, initial_nodes: Vec<NodeId>) -> Result<Vec<NodeId>, String> {
        let mut affected = HashSet::new();
        let mut queue = VecDeque::from(initial_nodes);

        let cache = self.cache.lock()?;

        while let Some(node_id) = queue.pop_front() {
            if affected.insert(node_id) {
                for edge in cache.edges.values() {
                    if edge.from_node_id == node_id && !affected.contains(&edge.to_node_id) {
                        queue.push_back(edge.to_node_id);
                    }
                }
            }
        }

        Ok(affected.into_iter().collect())
    }

    fn remove_edges_from_cache(&self, node_id: &NodeId) -> Result<(), String> {
        let mut cache = self.cache.lock()?;
        cache
            .edges
            .retain(|_, edge| edge.from_node_id != *node_id && edge.to_node_id != *node_id);
        Ok(())
    }

    fn remove_edge_from_cache(&self, edge_id: &EdgeId) -> Result<Edge, String> {
        let mut cache = self.cache.lock()?;
        cache
            .edges
            .remove(edge_id)
            .ok_or(format!("Edge does not exist: id: {:?}", edge_id))
    }

    fn mark_dirty_nodes(&mut self, nodes: Vec<NodeId>) {
        self.dirty_nodes.extend(nodes.into_iter());
    }
}
