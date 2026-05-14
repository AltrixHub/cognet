use crate::{Edge, EdgeId, ErrorTarget, GraphError, NodeGraph, NodePath};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) trait NodeGraphSystem {
    /// Topologically sort `target_paths`, returning levels of paths
    /// whose elements share no data dependencies on each other.
    fn topological_sort(
        &self,
        target_paths: &HashSet<NodePath>,
    ) -> Result<Vec<Vec<NodePath>>, String>;

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
                    // `remove` consumes the adjacency entry — each node is
                    // popped from the queue at most once during topo sort,
                    // so this is semantically equivalent to `get(..).cloned()`
                    // while avoiding a per-pop Vec<NodePath> allocation now
                    // that `NodePath: Clone` is no longer `Copy`.
                    let neighbors = adj_list.remove(&path).unwrap_or_default();
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

// ── Path-aware edge construction (plan-007 P007b) ──
//
// `create_edge_at` and `connect_nodes_at` accept `NodePath`s directly so
// callers can wire SubGraph-internal nodes (depth ≥ 2) without resorting
// to root-wrap workarounds. `NodeGraphWrite::connect_nodes` remains as a
// thin root-path wrapper; the prior `NodeGraphSystem::create_edge` has been
// removed (zero callers).

impl NodeGraph {
    /// Construct an `Edge` between two arbitrary `NodePath` endpoints.
    ///
    /// Resolves the slot ids for both endpoints from `NodeStates`. The
    /// returned `Edge` is not added to the graph; pass it to
    /// [`NodeGraphSystem::add_edge`] (or use [`connect_nodes_at`] which
    /// does both).
    pub fn create_edge_at(
        &self,
        from_path: &NodePath,
        from_output_slot_index: usize,
        to_path: &NodePath,
        to_input_slot_index: usize,
    ) -> Result<Edge, &'static str> {
        tracing::debug!("[cognet] create_edge_at: START");

        let ns = self
            .node_states
            .read()
            .map_err(|_| "Failed to read node states")?;

        let from_output_slot_id = ns
            .output_slot(from_path, from_output_slot_index)
            .ok_or("Invalid output slot index")?
            .id;

        let to_input_slot_id = ns
            .input_slot(to_path, to_input_slot_index)
            .ok_or("Invalid input slot index")?
            .id;

        drop(ns);

        tracing::debug!("[cognet] create_edge_at: DONE");
        Ok(Edge {
            from_node: from_path.clone(),
            from_output_slot_index,
            from_output_slot_id,
            to_node: to_path.clone(),
            to_input_slot_index,
            to_input_slot_id,
        })
    }

    /// Connect two nodes identified by `NodePath`. Path-aware sibling of
    /// [`NodeGraphWrite::connect_nodes`]; builds the `Edge` via
    /// [`create_edge_at`] and inserts it via
    /// [`NodeGraphSystem::add_edge`].
    pub fn connect_nodes_at(
        &mut self,
        from_path: &NodePath,
        from_output_slot_index: usize,
        to_path: &NodePath,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String> {
        let edge = self
            .create_edge_at(
                from_path,
                from_output_slot_index,
                to_path,
                to_input_slot_index,
            )
            .map_err(|e| e.to_string())?;

        self.add_edge(edge)
    }
}

#[cfg(test)]
mod path_aware_edge_tests {
    use crate::{NodeGraph, NodeGraphWrite, NodePath};

    #[test]
    fn connect_nodes_at_two_subgraph_children_creates_edge_at_depth_2() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = NodePath::root().child(sg_id);

        let num_id = graph
            .create_node_by_name_at(&sg_path, "Number")
            .expect("create num");
        let add_id = graph
            .create_node_by_name_at(&sg_path, "Add")
            .expect("create add");

        let from_path = sg_path.child(num_id);
        let to_path = sg_path.child(add_id);
        // Wire Number.Value (output 0) → Add.A (input 0).
        let edge_id = graph
            .connect_nodes_at(&from_path, 0, &to_path, 0)
            .expect("connect_nodes_at");

        let edge = graph.get_edge_by_id(&edge_id).expect("edge exists");
        assert_eq!(edge.from_node, from_path, "from_node must be depth-2 path");
        assert_eq!(edge.to_node, to_path, "to_node must be depth-2 path");
    }

    #[test]
    fn connect_nodes_root_wrapper_still_works() {
        let mut graph = NodeGraph::new().expect("create graph");
        let num_id = graph
            .create_node_by_name("Number")
            .expect("create_node_by_name num");
        let add_id = graph
            .create_node_by_name("Add")
            .expect("create_node_by_name add");

        let edge_id = graph
            .connect_nodes(&num_id, 0, &add_id, 0)
            .expect("connect_nodes root");

        let edge = graph.get_edge_by_id(&edge_id).expect("edge exists");
        assert_eq!(edge.from_node, NodePath::root().child(num_id));
        assert_eq!(edge.to_node, NodePath::root().child(add_id));
    }
}
