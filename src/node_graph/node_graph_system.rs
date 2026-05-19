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

    fn add_edge(&mut self, mut edge: Edge) -> Result<EdgeId, String> {
        tracing::debug!("[cognet] add_edge: START");

        // SubGraph external slots are an alias surface for the
        // InputProxy / OutputProxy InterfaceNode slots inside the
        // SubGraph. An edge whose endpoint is a SubGraph external slot
        // is transparently retargeted to the corresponding proxy slot,
        // so the executor's `resolve_value_through_interface`
        // follow-through finds the edge under the proxy slot at
        // execute time. Without this rewrite, edges to SubGraph
        // external slots are runtime no-ops (the SubGraphNode itself
        // does not execute and never reads its external slot values).
        //
        // `to_input_slot_id` / `from_output_slot_id` are pre-computed
        // caches that index the edge in NodeStates. After the path
        // rewrite we MUST re-resolve them from the proxy slot, or the
        // edge gets stored under the SubGraph external slot's id and
        // `edges_for_input` for the proxy returns empty — leaving the
        // follow-through with no incoming value at the proxy.
        if let Some((input_proxy_id, _)) = self.subgraph_proxy_ids_at_path(&edge.to_node) {
            let proxy_path = edge.to_node.child(input_proxy_id);
            let new_slot_id = {
                let ns = self.node_states.read().map_err(|e| e.to_string())?;
                ns.input_slot(&proxy_path, edge.to_input_slot_index)
                    .ok_or_else(|| {
                        format!(
                            "add_edge: InputProxy at {proxy_path} has no input slot \
                             {} (SubGraph external slot exists but the proxy is missing \
                             it — schema mismatch)",
                            edge.to_input_slot_index
                        )
                    })?
                    .id
            };
            edge.to_node = proxy_path;
            edge.to_input_slot_id = new_slot_id;
        }
        if let Some((_, output_proxy_id)) = self.subgraph_proxy_ids_at_path(&edge.from_node) {
            let proxy_path = edge.from_node.child(output_proxy_id);
            let new_slot_id = {
                let ns = self.node_states.read().map_err(|e| e.to_string())?;
                ns.output_slot(&proxy_path, edge.from_output_slot_index)
                    .ok_or_else(|| {
                        format!(
                            "add_edge: OutputProxy at {proxy_path} has no output slot \
                             {} (SubGraph external slot exists but the proxy is missing \
                             it — schema mismatch)",
                            edge.from_output_slot_index
                        )
                    })?
                    .id
            };
            edge.from_node = proxy_path;
            edge.from_output_slot_id = new_slot_id;
        }

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

#[cfg(test)]
mod subgraph_edge_alias_tests {
    //! Edges whose endpoint is a SubGraph external slot are transparently
    //! retargeted to the InterfaceNode proxy. These tests pin that
    //! contract so the SubGraph external port surface is a real data
    //! path (not a runtime no-op).

    use crate::{DataType, NodeGraph, NodeGraphWrite, NodeMeta, NodePath};

    /// Test-only passthrough node used by
    /// `execute_delivers_external_number_through_subgraph_input` (and
    /// future Phase X2 tests) to assert that an external Number value
    /// reaches a CONSUMER node downstream of `InputProxy`. We define
    /// our own node here (rather than reusing the built-in
    /// `"Number Output"` sink) so the assertion targets the consumer's
    /// own output slot — keeping the test valid in Phase X2 where the
    /// `InterfaceNode` tee becomes a runtime no-op and values flow via
    /// the executor's follow-through.
    #[derive(Debug)]
    struct NumberPassthrough;

    impl crate::NodeMeta for NumberPassthrough {
        const NAME: &'static str = "NumberPassthroughTest";
        const CATEGORY: crate::NodeCategory = crate::NodeCategory::Utility;
        const INPUTS: &'static [crate::SlotDef] = &[crate::SlotDef {
            label: "x",
            data_type: crate::DataType::Number,
            max_connections: Some(1),
        }];
        const OUTPUTS: &'static [crate::SlotDef] = &[crate::SlotDef {
            label: "x",
            data_type: crate::DataType::Number,
            max_connections: None,
        }];
        const DEFAULT_VALUE: crate::DefaultValue = crate::DefaultValue::None;
    }

    impl crate::NodeImpl for NumberPassthrough {
        fn execute_sync(&self, ctx: crate::ExecutionContext) -> Result<(), String> {
            if let Some(d) = ctx.input_values.first().and_then(|v| v.first()) {
                ctx.output_writer
                    .set(0, d.share())
                    .map_err(|e| format!("NumberPassthroughTest slot 0: {e}"))?;
            }
            Ok(())
        }
    }

    crate::register_nodes!(NumberPassthrough);

    #[test]
    fn input_edge_to_subgraph_external_slot_is_routed_to_input_proxy() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_input(&sg_id, "height", DataType::Number)
            .expect("add input");

        // External Number node at root.
        let num_id = graph.create_node_by_name("Number").expect("create Number");

        // Author connects external Number to the SubGraph's external "height" slot.
        let edge_id = graph
            .connect_nodes(&num_id, 0, &sg_id, 0)
            .expect("connect external to SubGraph external slot");

        // The edge MUST be stored as targeting the InputProxy at depth 2,
        // not the SubGraph at root.
        let edge = graph.get_edge_by_id(&edge_id).expect("edge exists");
        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let expected_to = NodePath::root().child(sg_id).child(input_proxy_id);
        assert_eq!(
            edge.to_node, expected_to,
            "edge target must be retargeted to InputProxy",
        );
        assert_eq!(edge.to_input_slot_index, 0);
    }

    #[test]
    fn output_edge_from_subgraph_external_slot_is_routed_from_output_proxy() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_output(&sg_id, "result", DataType::Number)
            .expect("add output");

        // External consumer at root.
        let consumer_id = graph.create_node_by_name("Add").expect("create Add");

        // Author connects SubGraph external "result" output to the consumer's input.
        let edge_id = graph
            .connect_nodes(&sg_id, 0, &consumer_id, 0)
            .expect("connect SubGraph external output to consumer");

        // The edge MUST be stored as sourced from the OutputProxy at depth 2.
        let edge = graph.get_edge_by_id(&edge_id).expect("edge exists");
        let (_, output_proxy_id) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let expected_from = NodePath::root().child(sg_id).child(output_proxy_id);
        assert_eq!(
            edge.from_node, expected_from,
            "edge source must be retargeted to OutputProxy",
        );
        assert_eq!(edge.from_output_slot_index, 0);
    }

    #[test]
    fn edge_between_two_subgraphs_rewrites_both_endpoints() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_a = graph
            .add_subgraph_at(&NodePath::root(), "A")
            .expect("add A");
        let sg_b = graph
            .add_subgraph_at(&NodePath::root(), "B")
            .expect("add B");
        graph
            .add_subgraph_output(&sg_a, "out", DataType::Number)
            .expect("add A output");
        graph
            .add_subgraph_input(&sg_b, "in", DataType::Number)
            .expect("add B input");

        let edge_id = graph
            .connect_nodes(&sg_a, 0, &sg_b, 0)
            .expect("connect A.out -> B.in");

        let edge = graph.get_edge_by_id(&edge_id).expect("edge exists");
        let (_, a_output_proxy) = graph.subgraph_proxy_ids(&sg_a).expect("A proxies");
        let (b_input_proxy, _) = graph.subgraph_proxy_ids(&sg_b).expect("B proxies");
        assert_eq!(
            edge.from_node,
            NodePath::root().child(sg_a).child(a_output_proxy)
        );
        assert_eq!(
            edge.to_node,
            NodePath::root().child(sg_b).child(b_input_proxy)
        );
    }

    /// Hypothesis: after the edge rewrite, the edge is stored under the
    /// SubGraph external slot's id (old `to_input_slot_id`), not the
    /// InputProxy slot's id. `edges_for_input` therefore returns the
    /// edge for the SubGraph external slot but EMPTY for the InputProxy
    /// slot. The executor reads InputProxy edges and finds nothing.
    ///
    /// This test pins the failure: after `connect_nodes`, asking for
    /// InputProxy.input[0]'s incoming edges should return ONE edge
    /// (the routed one). Currently it returns zero, proving the
    /// `to_input_slot_id` is stale.
    #[test]
    fn routed_edge_is_indexed_under_input_proxy_slot_id_not_subgraph_external() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_input(&sg_id, "x", DataType::Number)
            .expect("add input");

        let num_id = graph.create_node_by_name("Number").expect("create Number");

        let edge_id = graph
            .connect_nodes(&num_id, 0, &sg_id, 0)
            .expect("connect external -> SubGraph external slot");

        let edge = graph.get_edge_by_id(&edge_id).expect("edge stored");
        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let proxy_path = NodePath::root().child(sg_id).child(input_proxy_id);

        // edge.to_node was rewritten to InputProxy (verified by other tests).
        assert_eq!(edge.to_node, proxy_path);

        // The InputProxy slot's id (live, looked up now) vs the edge's
        // stored slot id.
        let ns = graph.node_states().read().expect("read");
        let input_proxy_slot = ns
            .input_slot(&proxy_path, 0)
            .expect("InputProxy slot exists");
        assert_eq!(
            edge.to_input_slot_id, input_proxy_slot.id,
            "edge's to_input_slot_id MUST match the InputProxy slot's id; \
             currently it still points at the SubGraph external slot, so \
             edges_for_input(proxy_slot.id) returns empty and the executor \
             sees no incoming value at the proxy."
        );
    }

    /// Hypothesis (symmetric to the InputProxy test): after the edge
    /// rewrite, the edge from a SubGraph external output slot must be
    /// stored under the OutputProxy slot's id (the live
    /// `from_output_slot_id`), not the SubGraph external slot's id.
    /// Otherwise `edges_for_output` for the OutputProxy slot returns
    /// empty and the executor sees no outgoing edge from the proxy.
    ///
    /// This test pins the cache contract on the output side so a future
    /// refactor that touches only the `from_*` branch of `add_edge`
    /// cannot silently regress the cache.
    #[test]
    fn routed_edge_is_indexed_under_output_proxy_slot_id_not_subgraph_external() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_output(&sg_id, "y", DataType::Number)
            .expect("add output");

        let consumer_id = graph.create_node_by_name("Add").expect("create Add");

        let edge_id = graph
            .connect_nodes(&sg_id, 0, &consumer_id, 0)
            .expect("connect SubGraph external output -> consumer");

        let edge = graph.get_edge_by_id(&edge_id).expect("edge stored");
        let (_, output_proxy_id) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let proxy_path = NodePath::root().child(sg_id).child(output_proxy_id);

        // edge.from_node was rewritten to OutputProxy (verified by other tests).
        assert_eq!(edge.from_node, proxy_path);

        // The OutputProxy slot's id (live, looked up now) vs the edge's
        // stored slot id.
        let ns = graph.node_states().read().expect("read");
        let output_proxy_slot = ns
            .output_slot(&proxy_path, 0)
            .expect("OutputProxy slot exists");
        assert_eq!(
            edge.from_output_slot_id, output_proxy_slot.id,
            "edge's from_output_slot_id MUST match the OutputProxy slot's id; \
             if it still points at the SubGraph external slot, \
             edges_for_output(proxy_slot.id) returns empty and the executor \
             sees no outgoing edge from the proxy."
        );
    }

    #[test]
    fn edges_to_non_subgraph_nodes_are_unchanged() {
        let mut graph = NodeGraph::new().expect("create graph");
        let num_id = graph.create_node_by_name("Number").expect("num");
        let add_id = graph.create_node_by_name("Add").expect("add");

        let edge_id = graph
            .connect_nodes(&num_id, 0, &add_id, 0)
            .expect("connect");

        let edge = graph.get_edge_by_id(&edge_id).expect("edge exists");
        assert_eq!(edge.from_node, NodePath::root().child(num_id));
        assert_eq!(edge.to_node, NodePath::root().child(add_id));
    }

    /// End-to-end proof of Option D's contract: after `add_edge` rewrites
    /// a Number → SubGraph external edge into a Number → InputProxy edge,
    /// an `execute_sync()` cycle delivers the external Number's value to
    /// a CONSUMER node (`NumberPassthroughTest`, defined above) wired
    /// downstream of InputProxy *inside* the SubGraph. The assertion
    /// deliberately checks the consumer's output — NOT the InterfaceNode
    /// tee's output — so the test survives Phase X2 (where the tee
    /// becomes a runtime no-op and values flow via
    /// `resolve_value_through_interface`).
    #[test]
    fn execute_delivers_external_number_through_subgraph_input() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_input(&sg_id, "x", DataType::Number)
            .expect("add input");

        // Wire an internal consumer: InputProxy.output[0] -> consumer.input[0].
        // The consumer's own output[0] is what we assert on.
        let sg_path = NodePath::root().child(sg_id);
        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let proxy_path = sg_path.child(input_proxy_id);

        let consumer_id = graph
            .create_node_by_name_at(&sg_path, NumberPassthrough::NAME)
            .expect("create consumer");
        let consumer_path = sg_path.child(consumer_id);
        graph
            .connect_nodes_at(&proxy_path, 0, &consumer_path, 0)
            .expect("internal: InputProxy -> consumer");

        // External: Number(7.5) -> SubGraph.external[0]
        // (gets rewritten by add_edge to Number -> InputProxy.input[0]).
        let num_id = graph.create_node_by_name("Number").expect("create Number");
        graph
            .update_node_data(&num_id, crate::Data::new(7.5_f64).expect("Data::new f64"))
            .expect("set Number data");
        graph
            .connect_nodes(&num_id, 0, &sg_id, 0)
            .expect("connect external -> SubGraph external slot");

        let result = graph.execute_sync().expect("execute_sync");

        let consumer_outputs = result
            .node_outputs
            .get(&consumer_path)
            .expect("consumer node must appear in execution result");
        let first = consumer_outputs
            .first()
            .and_then(|o| o.as_ref())
            .expect("consumer must produce an output for slot 0");
        let f: f64 = *first.value::<f64>().expect("Number downcast");
        assert!(
            (f - 7.5).abs() < 1e-9,
            "consumer downstream of InputProxy must see external Number's 7.5; got {f}"
        );
    }

    /// Option D contract: with InterfaceNode runtime-skipped, an internal
    /// consumer of `InputProxy.output[i]` sees the external Number's value
    /// even though InterfaceNode itself does NOT execute (its output cache
    /// is never written to). This pins the executor's follow-through.
    #[test]
    fn internal_consumer_sees_external_value_with_interface_node_skipped() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_input(&sg_id, "x", DataType::Number)
            .expect("add input");

        let sg_path = NodePath::root().child(sg_id);
        // Reuse the `NumberPassthrough` defined in Task X1.2 (this test
        // module is the same `subgraph_edge_alias_tests`). The X2 contract
        // requires a real output-producing node; "Number Output" alone
        // would not exercise the follow-through.
        let consumer_id = graph
            .create_node_by_name_at(&sg_path, NumberPassthrough::NAME)
            .expect("create consumer");
        let consumer_path = sg_path.child(consumer_id);

        // Author wiring: InputProxy.output[0] -> consumer.input[0].
        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let proxy_path = sg_path.child(input_proxy_id);
        graph
            .connect_nodes_at(&proxy_path, 0, &consumer_path, 0)
            .expect("internal author wiring");

        // External: Number(11.0) -> SubGraph.external[0]  (rewritten to InputProxy.input[0]).
        let num_id = graph.create_node_by_name("Number").expect("create Number");
        graph
            .update_node_data(&num_id, crate::Data::new(11.0_f64).expect("Data::new"))
            .expect("set data");
        graph
            .connect_nodes(&num_id, 0, &sg_id, 0)
            .expect("external edge");

        let result = graph.execute_sync().expect("execute_sync");

        // The InterfaceNode (InputProxy) should have NOT executed — its output
        // entry should be absent OR its output slot value should be None (the
        // tee is a no-op).
        let proxy_output = result.node_outputs.get(&proxy_path);
        assert!(
            proxy_output.is_none_or(|outs| outs.iter().all(Option::is_none)),
            "InterfaceNode must not produce output values at runtime; got: {:?}",
            proxy_output
        );

        // The internal consumer should see the external Number's value
        // (resolved through the InterfaceNode follow-through).
        let consumer_output = result
            .node_outputs
            .get(&consumer_path)
            .expect("consumer ran");
        let first = consumer_output
            .first()
            .and_then(|o| o.as_ref())
            .expect("consumer produced an output value");
        let v: f64 = *first.value::<f64>().expect("Number downcast");
        assert!(
            (v - 11.0).abs() < 1e-9,
            "internal consumer must see external Number's value via the \
             InterfaceNode follow-through; got {v}"
        );
    }

    /// Bug fix: a `Number Output` sink (no output slots) connected to a
    /// SubGraph external OUTPUT port must see the upstream value carried
    /// through the OutputProxy follow-through. Before this fix,
    /// `collect_outputs`'s sink-input branch read the cache directly with
    /// `cache.outputs.get(edge.from_output_slot_id)`, which misses because
    /// `OutputProxy` is a Phase X2 runtime no-op and never writes its own
    /// output cache. The sink would then appear in `node_outputs` with
    /// `None` for slot 0 and the UI would display "--".
    #[test]
    fn number_output_sink_sees_value_from_subgraph_output_via_follow_through() {
        let mut graph = NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_output(&sg_id, "y", DataType::Number)
            .expect("add_subgraph_output");

        let sg_path = NodePath::root().child(sg_id);
        let (_, output_proxy_id) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let output_proxy_path = sg_path.child(output_proxy_id);

        // Internal: Number(42.0) -> OutputProxy.input[0]
        let inner_num_id = graph
            .create_node_by_name_at(&sg_path, "Number")
            .expect("create internal Number");
        graph
            .update_node_data_at(
                &sg_path.child(inner_num_id),
                crate::Data::new(42.0_f64).expect("Data::new"),
            )
            .expect("set Number data");
        graph
            .connect_nodes_at(&sg_path.child(inner_num_id), 0, &output_proxy_path, 0)
            .expect("internal: Number -> OutputProxy input");

        // External: SubGraph.y -> Number Output.input[0]
        // (the edge gets rewritten by add_edge to OutputProxy -> Number Output)
        let sink_id = graph
            .create_node_by_name("Number Output")
            .expect("create Number Output sink");
        graph
            .connect_nodes(&sg_id, 0, &sink_id, 0)
            .expect("external: SubGraph.y -> Number Output");

        let result = graph.execute_sync().expect("execute_sync");

        // The sink's node_outputs entry should carry the upstream Number's value.
        let sink_outputs = result
            .node_outputs
            .get(&NodePath::root().child(sink_id))
            .expect("Number Output sink must appear in execution result");
        let first = sink_outputs
            .first()
            .and_then(|o| o.as_ref())
            .expect("sink must have an input-derived value");
        let v: f64 = *first.value::<f64>().expect("Number downcast");
        assert!(
            (v - 42.0).abs() < 1e-9,
            "Number Output sink must see the internal Number(42.0) routed \
             through OutputProxy follow-through; got {v}"
        );
    }
}
