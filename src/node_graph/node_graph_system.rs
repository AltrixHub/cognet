use crate::{Edge, EdgeId, ErrorTarget, GraphError, NodeGraph, NodeGraphAPI, NodeId};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) trait NodeGraphSystem {
    fn topological_sort(&self, target_nodes: &HashSet<NodeId>) -> Result<Vec<Vec<NodeId>>, String>;

    fn create_edge(
        &self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str>;

    fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String>;

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

    fn create_edge(
        &self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str> {
        tracing::debug!("[cognet] create_edge: START");

        let from_output_slot_id = {
            tracing::debug!("[cognet] create_edge: getting from_node...");
            let node = self
                .get_node_by_id(from_node_id)
                .ok_or("From node not found")?;
            tracing::debug!("[cognet] create_edge: got from_node, acquiring read lock...");
            let read_node = node.read().map_err(|_| "Failed to acquire read lock")?;
            tracing::debug!("[cognet] create_edge: from_node read lock acquired");
            read_node
                .outputs()
                .get(from_output_slot_index)
                .ok_or("Invalid output slot index")?
                .id
                .clone()
        };

        let to_input_slot_id = {
            tracing::debug!("[cognet] create_edge: getting to_node...");
            let node = self
                .get_node_by_id(to_node_id)
                .ok_or("To node not found")?;
            tracing::debug!("[cognet] create_edge: got to_node, acquiring read lock...");
            let read_node = node.read().map_err(|_| "Failed to acquire read lock")?;
            tracing::debug!("[cognet] create_edge: to_node read lock acquired");
            read_node
                .inputs()
                .get(to_input_slot_index)
                .ok_or("Invalid input slot index")?
                .id
                .clone()
        };

        tracing::debug!("[cognet] create_edge: DONE");
        Ok(Edge {
            from_node_id: *from_node_id,
            from_output_slot_index,
            from_output_slot_id,
            to_node_id: *to_node_id,
            to_input_slot_index,
            to_input_slot_id,
        })
    }

    fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String> {
        tracing::debug!("[cognet] add_edge: START");
        let from_node_id = edge.from_node_id;
        let to_node_id = edge.to_node_id;

        // Clear previous errors for the target input port
        let input_target = ErrorTarget::InputPort {
            node_id: to_node_id,
            slot_index: edge.to_input_slot_index,
        };
        self.clear_error(&input_target);

        tracing::debug!("[cognet] add_edge: getting from_node...");
        let from_node = match self.node_manager.get_node_by_id(&from_node_id) {
            Some(node) => node,
            None => {
                let error = GraphError::node_not_found(from_node_id);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }
        };
        tracing::debug!("[cognet] add_edge: got from_node");

        tracing::debug!("[cognet] add_edge: getting to_node...");
        let to_node = match self.node_manager.get_node_by_id(&to_node_id) {
            Some(node) => node,
            None => {
                let error = GraphError::node_not_found(to_node_id);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }
        };
        tracing::debug!("[cognet] add_edge: got to_node");

        tracing::debug!("[cognet] add_edge: acquiring from_node read lock...");
        let read_from_node = from_node.read().map_err(|e| e.to_string())?;
        tracing::debug!("[cognet] add_edge: from_node read lock acquired");
        let from_slot = match read_from_node.get_output_slot_by_index(edge.from_output_slot_index)
        {
            Some(slot) => slot,
            None => {
                let error =
                    GraphError::output_slot_not_found(from_node_id, edge.from_output_slot_index);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }
        };

        tracing::debug!("[cognet] add_edge: acquiring to_node read lock...");
        let read_to_node = to_node.read().map_err(|e| e.to_string())?;
        tracing::debug!("[cognet] add_edge: to_node read lock acquired");
        let to_slot = match read_to_node.get_input_slot_by_index(edge.to_input_slot_index) {
            Some(slot) => slot,
            None => {
                let error =
                    GraphError::input_slot_not_found(to_node_id, edge.to_input_slot_index);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }
        };

        // Check type compatibility
        if from_slot.data_type != to_slot.data_type {
            let error = GraphError::type_mismatch(
                to_node_id,
                edge.to_input_slot_index,
                from_slot.data_type,
                to_slot.data_type,
            );
            let msg = error.message();
            self.add_error(error);
            return Err(msg);
        }

        // Check max_connections limit BEFORE adding the edge
        if let Some(max) = to_slot.max_connections() {
            let current_count = self
                .state_access
                .storage()
                .read()
                .ok()
                .map(|s| {
                    s.input_slot(&to_node_id, edge.to_input_slot_index)
                        .map(|slot| slot.connected_edges.len())
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            if current_count >= *max {
                let error = GraphError::connection_limit_exceeded(
                    to_node_id,
                    edge.to_input_slot_index,
                    *max,
                );
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }
        }

        // Release the read locks before acquiring write lock
        drop(read_from_node);
        drop(read_to_node);

        let edge_id = EdgeId::new();

        // Update NodeStates slot connections and add edge via state access
        {
            let mut guard = self
                .state_access
                .storage()
                .write()
                .map_err(|e| e.to_string())?;
            if let Some(slot_state) =
                guard.output_slot_mut(&from_node_id, edge.from_output_slot_index)
            {
                slot_state.connected_edges.push(edge_id);
            }
            if let Some(slot_state) =
                guard.input_slot_mut(&to_node_id, edge.to_input_slot_index)
            {
                slot_state.connected_edges.push(edge_id);
            }
            guard.add_edge(edge_id, edge.clone());
        }

        tracing::debug!("[cognet] add_edge: collecting dirty nodes...");
        let dirty_nodes = self.collect_dirty_nodes(vec![from_node_id, to_node_id])?;
        tracing::debug!("[cognet] add_edge: marking dirty nodes...");
        self.mark_dirty_nodes(dirty_nodes);
        tracing::debug!("[cognet] add_edge: acquiring cache lock...");
        let mut cache = self.cache.lock()?;
        cache.add_edge(edge_id, edge);
        tracing::debug!("[cognet] add_edge: DONE");

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
            .remove_edge(edge_id)
            .ok_or(format!("Edge does not exist: id: {:?}", edge_id))
    }

    fn mark_dirty_nodes(&mut self, nodes: Vec<NodeId>) {
        self.dirty_nodes.extend(nodes.into_iter());
    }
}
