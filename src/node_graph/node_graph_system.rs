use crate::{Edge, EdgeId, ErrorTarget, GraphError, NodeGraph, NodeId};
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

    fn mark_dirty_nodes(&self, nodes: Vec<NodeId>);

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

        let ns = self.node_states.read().map_err(|e| e.to_string())?;
        for node_id in target_nodes {
            for edge_id in ns.outgoing_edges_for_node(node_id) {
                if let Some(edge) = ns.get_edge(edge_id) {
                    if target_nodes.contains(&edge.to_node_id) {
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

        let ns = self
            .node_states
            .read()
            .map_err(|_| "Failed to read node states")?;

        let from_output_slot_id = ns
            .output_slot(from_node_id, from_output_slot_index)
            .ok_or("Invalid output slot index")?
            .id;

        let to_input_slot_id = ns
            .input_slot(to_node_id, to_input_slot_index)
            .ok_or("Invalid input slot index")?
            .id;

        drop(ns);

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

        // Validate slots and check constraints using NodeStates
        {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;

            // Validate from node exists
            if ns.get(&from_node_id).is_none() {
                let error = GraphError::node_not_found(from_node_id);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }

            // Validate to node exists
            if ns.get(&to_node_id).is_none() {
                let error = GraphError::node_not_found(to_node_id);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }

            // Validate output slot
            let from_slot = match ns.output_slot(&from_node_id, edge.from_output_slot_index) {
                Some(slot) => slot,
                None => {
                    let error = GraphError::output_slot_not_found(
                        from_node_id,
                        edge.from_output_slot_index,
                    );
                    let msg = error.message();
                    self.add_error(error);
                    return Err(msg);
                }
            };

            // Validate input slot
            let to_slot = match ns.input_slot(&to_node_id, edge.to_input_slot_index) {
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
            if let Some(max) = to_slot.max_connections {
                if to_slot.connected_edges.len() >= max {
                    let error = GraphError::connection_limit_exceeded(
                        to_node_id,
                        edge.to_input_slot_index,
                        max,
                    );
                    let msg = error.message();
                    self.add_error(error);
                    return Err(msg);
                }
            }
        }

        let edge_id = EdgeId::new();

        // Update NodeStates (edge storage + slot connected_edges + indexes)
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_edge(edge_id, edge.clone());
        }

        tracing::debug!("[cognet] add_edge: collecting dirty nodes...");
        let dirty_nodes = self.collect_dirty_nodes(vec![from_node_id, to_node_id])?;
        tracing::debug!("[cognet] add_edge: marking dirty nodes...");
        self.mark_dirty_nodes(dirty_nodes);
        Ok(edge_id)
    }

    fn collect_dirty_nodes(&self, initial_nodes: Vec<NodeId>) -> Result<Vec<NodeId>, String> {
        let mut affected = HashSet::new();
        let mut queue = VecDeque::from(initial_nodes);

        let ns = self.node_states.read().map_err(|e| e.to_string())?;

        while let Some(node_id) = queue.pop_front() {
            if affected.insert(node_id) {
                for edge_id in ns.outgoing_edges_for_node(&node_id) {
                    if let Some(edge) = ns.get_edge(edge_id) {
                        if !affected.contains(&edge.to_node_id) {
                            queue.push_back(edge.to_node_id);
                        }
                    }
                }
            }
        }

        Ok(affected.into_iter().collect())
    }

    fn remove_edges_from_cache(&self, node_id: &NodeId) -> Result<(), String> {
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.remove_edges_for_node(node_id);
        Ok(())
    }

    fn remove_edge_from_cache(&self, edge_id: &EdgeId) -> Result<Edge, String> {
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.remove_edge(edge_id)
            .ok_or(format!("Edge does not exist: id: {:?}", edge_id))
    }

    fn mark_dirty_nodes(&self, nodes: Vec<NodeId>) {
        self.mark_dirty(nodes);
    }
}
