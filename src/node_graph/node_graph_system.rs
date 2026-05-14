use crate::{Edge, EdgeId, ErrorTarget, GraphError, NodeGraph, NodeId, NodePath};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) trait NodeGraphSystem {
    /// Topologically sort `target_paths`, returning levels of paths
    /// whose elements share no data dependencies on each other.
    fn topological_sort(
        &self,
        target_paths: &HashSet<NodePath>,
    ) -> Result<Vec<Vec<NodePath>>, String>;

    /// Build an `Edge` value from node IDs and slot indices.
    ///
    /// Constructs root-level `NodePath`s for both endpoints. When the
    /// caller has real (non-root) paths available it should construct
    /// the `Edge` directly instead.
    fn create_edge(
        &self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str>;

    fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String>;
}

impl NodeGraphSystem for NodeGraph {
    fn topological_sort(
        &self,
        target_paths: &HashSet<NodePath>,
    ) -> Result<Vec<Vec<NodePath>>, String> {
        let mut in_degree: HashMap<NodePath, usize> = HashMap::new();
        let mut adj_list: HashMap<NodePath, Vec<NodePath>> = HashMap::new();

        for path in target_paths {
            in_degree.insert(path.clone(), 0);
            adj_list.insert(path.clone(), Vec::new());
        }

        let ns = self.node_states.read().map_err(|e| e.to_string())?;
        for target_path in target_paths {
            for edge_id in ns.outgoing_edges_at(target_path) {
                if let Some(edge) = ns.get_edge(edge_id) {
                    if target_paths.contains(&edge.to_node) {
                        in_degree
                            .entry(edge.to_node.clone())
                            .and_modify(|count| *count += 1)
                            .or_insert(1);
                        adj_list
                            .entry(edge.from_node.clone())
                            .or_default()
                            .push(edge.to_node.clone());
                    }
                }
            }
        }

        let mut queue: VecDeque<NodePath> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(path, _)| path.clone())
            .collect();

        let mut sorted = Vec::new();

        while !queue.is_empty() {
            let mut current_level = Vec::new();

            for _ in 0..queue.len() {
                if let Some(path) = queue.pop_front() {
                    let neighbors = adj_list.get(&path).cloned().unwrap_or_default();
                    current_level.push(path);

                    for neighbor in &neighbors {
                        if let Some(deg) = in_degree.get_mut(neighbor) {
                            *deg -= 1;
                            if *deg == 0 {
                                queue.push_back(neighbor.clone());
                            }
                        }
                    }
                }
            }

            sorted.push(current_level);
        }

        let total_count: usize = sorted.iter().map(|level| level.len()).sum();
        if total_count != target_paths.len() {
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
            .output_slot(
                &NodePath::root().child(*from_node_id),
                from_output_slot_index,
            )
            .ok_or("Invalid output slot index")?
            .id;

        let to_input_slot_id = ns
            .input_slot(&NodePath::root().child(*to_node_id), to_input_slot_index)
            .ok_or("Invalid input slot index")?
            .id;

        drop(ns);

        tracing::debug!("[cognet] create_edge: DONE");
        Ok(Edge {
            from_node: NodePath::root().child(*from_node_id),
            from_output_slot_index,
            from_output_slot_id,
            to_node: NodePath::root().child(*to_node_id),
            to_input_slot_index,
            to_input_slot_id,
        })
    }

    fn add_edge(&mut self, edge: Edge) -> Result<EdgeId, String> {
        tracing::debug!("[cognet] add_edge: START");
        let from_node = edge.from_node.clone();
        let to_node = edge.to_node.clone();
        // For error reporting we need a NodeId. Leaf() is always Some for
        // non-root paths; root-level nodes have a single-element path.
        let from_node_id = from_node
            .leaf()
            .ok_or_else(|| "add_edge: from_node is a root path".to_string())?;
        let to_node_id = to_node
            .leaf()
            .ok_or_else(|| "add_edge: to_node is a root path".to_string())?;

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
            if ns.get(&from_node).is_none() {
                let error = GraphError::node_not_found(from_node_id);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }

            // Validate to node exists
            if ns.get(&to_node).is_none() {
                let error = GraphError::node_not_found(to_node_id);
                let msg = error.message();
                self.add_error(error);
                return Err(msg);
            }

            // Validate output slot
            let from_slot = match ns.output_slot(&from_node, edge.from_output_slot_index) {
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
            let to_slot = match ns.input_slot(&to_node, edge.to_input_slot_index) {
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
                let current = ns.edges_for_input(&to_slot.id).len();
                if current >= max {
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

        // Update NodeStates (edge storage + indexes + auto-tracks changed)
        {
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            guard.add_edge(edge_id, edge.clone());
        }

        Ok(edge_id)
    }
}
