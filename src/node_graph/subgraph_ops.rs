//! SubGraph operations: group_nodes and ungroup_node.
//!
//! These operations allow grouping selected nodes into a SubGraphNode
//! and ungrouping them back into the parent graph.

use std::collections::{HashMap, HashSet};

use crate::{
    DataType, Edge, EdgeId, InputSlot, NodeGraph, NodeGraphAPI, NodeId, OutputSlot, SubGraphNode,
};

/// Information about an edge crossing the subgraph boundary.
#[derive(Debug)]
struct BoundaryEdge {
    edge_id: EdgeId,
    edge: Edge,
}

/// Result of grouping nodes into a subgraph.
#[derive(Debug)]
pub struct GroupResult {
    /// The NodeId of the new SubGraphNode in the parent graph.
    pub subgraph_node_id: NodeId,
    /// Mapping from original node IDs to internal node IDs inside the subgraph.
    /// Used for copying positions when expanding into the subgraph view.
    pub node_id_remap: HashMap<NodeId, NodeId>,
}

/// Result of ungrouping a subgraph node.
#[derive(Debug)]
pub struct UngroupResult {
    /// NodeIds of nodes that were extracted from the subgraph back into the parent.
    pub extracted_node_ids: Vec<NodeId>,
}

impl NodeGraph {
    /// Group a set of nodes into a SubGraphNode.
    ///
    /// Automatically detects external I/O edges:
    /// - Edges from outside → selected nodes become SubGraphNode inputs
    /// - Edges from selected nodes → outside become SubGraphNode outputs
    /// - Internal edges (both endpoints selected) are preserved inside the subgraph
    ///
    /// Returns the NodeId of the newly created SubGraphNode.
    pub fn group_nodes(
        &mut self,
        node_ids: &HashSet<NodeId>,
        label: impl Into<String>,
    ) -> Result<GroupResult, String> {
        if node_ids.is_empty() {
            return Err("Cannot group empty set of nodes".to_string());
        }

        // 1. Classify edges into: incoming, outgoing, internal
        let (incoming_edges, outgoing_edges, internal_edges) =
            self.classify_boundary_edges(node_ids)?;

        // 2. Filter outgoing boundary edges: skip "intermediate" outputs.
        //    If an output slot (from_node, from_slot) has BOTH internal consumers
        //    (edges to selected nodes) AND external consumers (outgoing edges), the
        //    data already flows through the internal chain. The external connections
        //    are intermediate taps that clutter the SubGraphNode interface.
        //    Only expose output ports for slots whose data is exclusively consumed
        //    externally.
        //    NOTE: Incoming edges are NOT filtered. Multi-input slots may have both
        //    external and internal sources, and filtering would remove valid external
        //    connections.
        let internal_source_slots: HashSet<(NodeId, usize)> = internal_edges
            .iter()
            .map(|e| (e.edge.from_node_id, e.edge.from_output_slot_index))
            .collect();

        // Keep only outgoing edges whose source slot has NO internal consumers
        let (outgoing_edges, dropped_outgoing): (Vec<_>, Vec<_>) =
            outgoing_edges.into_iter().partition(|e| {
                !internal_source_slots
                    .contains(&(e.edge.from_node_id, e.edge.from_output_slot_index))
            });

        // Incoming edges: no filter (all external sources are valid inputs)
        let dropped_incoming: Vec<BoundaryEdge> = Vec::new();

        // 3. Deduplicate boundary edges by source/target slot.
        //    Multiple edges from the same output slot share one SubGraphNode port.
        //    Multiple edges to the same input slot share one SubGraphNode port.
        let deduped_outgoing = Self::dedup_outgoing(&outgoing_edges);
        let deduped_incoming = Self::dedup_incoming(&incoming_edges);

        tracing::debug!(
            "group_nodes: selected={} internal={} outgoing={} (dropped={}) incoming={} (dropped={}) deduped_out={} deduped_in={}",
            node_ids.len(),
            internal_edges.len(),
            outgoing_edges.len(),
            dropped_outgoing.len(),
            incoming_edges.len(),
            dropped_incoming.len(),
            deduped_outgoing.len(),
            deduped_incoming.len(),
        );

        // 4. Create SubGraphNode
        let subgraph_id = self.create_node::<SubGraphNode>()?;

        // Get mutable access to the SubGraphNode
        let subgraph_entity = self
            .get_node_by_id(&subgraph_id)
            .ok_or("SubGraphNode not found after creation")?;
        let mut write_subgraph = subgraph_entity.write().map_err(|e| e.to_string())?;
        let subgraph = write_subgraph
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Failed to downcast to SubGraphNode")?;

        subgraph.set_label(label);

        // 5. Add input slots (one per unique source slot)
        let mut input_slot_map: HashMap<EdgeId, usize> = HashMap::new();
        for (idx, group) in deduped_incoming.iter().enumerate() {
            let first = &group[0];
            let data_type = self.get_edge_data_type(&first.edge)?;
            // Use source node's output slot label (e.g. Vertex→"Position", Number→"Value")
            let source_label = self
                .get_node_by_id(&first.edge.from_node_id)
                .and_then(|e| {
                    e.read().ok().and_then(|g| {
                        g.get_output_slot_by_index(first.edge.from_output_slot_index)
                            .map(|s| s.label)
                    })
                })
                .unwrap_or("");
            let input_slot = InputSlot {
                label: source_label,
                data_type,
                ..Default::default()
            };
            subgraph.add_input(input_slot)?;
            for edge in group {
                input_slot_map.insert(edge.edge_id, idx);
            }
        }

        // 6. Add output slots (one per unique source slot from outgoing edges)
        let mut output_slot_map: HashMap<EdgeId, usize> = HashMap::new();
        for (idx, group) in deduped_outgoing.iter().enumerate() {
            let first = &group[0];
            let data_type = self.get_edge_data_type(&first.edge)?;
            let source_label = self
                .get_node_by_id(&first.edge.from_node_id)
                .and_then(|e| {
                    e.read().ok().and_then(|g| {
                        g.get_output_slot_by_index(first.edge.from_output_slot_index)
                            .map(|s| s.label)
                    })
                })
                .unwrap_or("");
            let output_slot = OutputSlot {
                label: source_label,
                data_type,
                ..Default::default()
            };
            subgraph.add_output(output_slot)?;
            for edge in group {
                output_slot_map.insert(edge.edge_id, idx);
            }
        }

        // 6b. Detect terminal outputs: output slots with no consumers at all.
        //     These are slots that have neither internal consumers (no internal edges
        //     from this slot) nor external consumers (no outgoing edges from this slot).
        //     Example: Merge node at the end of a chain with no downstream node.
        let consumed_slots: HashSet<(NodeId, usize)> = internal_edges
            .iter()
            .map(|e| (e.edge.from_node_id, e.edge.from_output_slot_index))
            .chain(
                outgoing_edges
                    .iter()
                    .map(|e| (e.edge.from_node_id, e.edge.from_output_slot_index)),
            )
            .chain(
                dropped_outgoing
                    .iter()
                    .map(|e| (e.edge.from_node_id, e.edge.from_output_slot_index)),
            )
            .collect();

        struct TerminalOutput {
            node_id: NodeId,
            slot_index: usize,
            data_type: DataType,
            label: &'static str,
        }

        let mut terminal_outputs: Vec<TerminalOutput> = Vec::new();
        for &node_id in node_ids {
            if let Some(node_entity) = self.get_node_by_id(&node_id) {
                if let Ok(read_node) = node_entity.read() {
                    for (slot_idx, slot) in read_node.outputs().iter().enumerate() {
                        if !consumed_slots.contains(&(node_id, slot_idx))
                            && !slot.label.starts_with('_')
                        {
                            terminal_outputs.push(TerminalOutput {
                                node_id,
                                slot_index: slot_idx,
                                data_type: slot.data_type,
                                label: slot.label,
                            });
                        }
                    }
                }
            }
        }

        // Add terminal outputs as additional SubGraphNode output slots
        let terminal_start_idx = deduped_outgoing.len();
        let mut terminal_slot_map: Vec<(NodeId, usize, usize)> = Vec::new();
        for (i, terminal) in terminal_outputs.iter().enumerate() {
            let output_slot = OutputSlot {
                label: terminal.label,
                data_type: terminal.data_type,
                ..Default::default()
            };
            subgraph.add_output(output_slot)?;
            terminal_slot_map.push((terminal.node_id, terminal.slot_index, terminal_start_idx + i));
        }

        tracing::debug!(
            "group_nodes: terminal_outputs={}",
            terminal_outputs.len(),
        );

        let input_proxy_id = subgraph.input_proxy_id();
        let output_proxy_id = subgraph.output_proxy_id();
        let internal_graph = subgraph.internal_graph_mut();

        // 7. Create copies of selected nodes inside the internal graph
        let mut node_id_remap: HashMap<NodeId, NodeId> = HashMap::new();
        for &node_id in node_ids {
            if let Some(node_entity) = self.get_node_by_id(&node_id) {
                let read_node = node_entity.read().map_err(|e| e.to_string())?;
                let type_name = read_node.node_name();
                drop(read_node);

                let internal_id = internal_graph.create_node_by_name(type_name)?;

                // Copy node data
                if let Some(original) = self.get_node_by_id(&node_id) {
                    let read_original = original.read().map_err(|e| e.to_string())?;
                    if let Some(data) = read_original.node_data() {
                        drop(read_original);
                        internal_graph.update_node_data(&internal_id, data)?;
                    }
                }

                node_id_remap.insert(node_id, internal_id);
            }
        }

        // 8. Recreate internal edges inside the subgraph
        for boundary in &internal_edges {
            let from_internal = node_id_remap
                .get(&boundary.edge.from_node_id)
                .ok_or("Internal edge source not in remap")?;
            let to_internal = node_id_remap
                .get(&boundary.edge.to_node_id)
                .ok_or("Internal edge target not in remap")?;
            internal_graph.connect_nodes(
                from_internal,
                boundary.edge.from_output_slot_index,
                to_internal,
                boundary.edge.to_input_slot_index,
            )?;
        }

        // 9. Connect incoming edges: InputProxy → internal target
        for boundary in &incoming_edges {
            let proxy_output_idx = input_slot_map[&boundary.edge_id];
            let internal_target = node_id_remap
                .get(&boundary.edge.to_node_id)
                .ok_or("Incoming edge target not in remap")?;
            internal_graph.connect_nodes(
                &input_proxy_id,
                proxy_output_idx,
                internal_target,
                boundary.edge.to_input_slot_index,
            )?;
        }

        // 10. Connect outgoing edges: internal source → OutputProxy
        for boundary in &outgoing_edges {
            let proxy_input_idx = output_slot_map[&boundary.edge_id];
            let internal_source = node_id_remap
                .get(&boundary.edge.from_node_id)
                .ok_or("Outgoing edge source not in remap")?;
            internal_graph.connect_nodes(
                internal_source,
                boundary.edge.from_output_slot_index,
                &output_proxy_id,
                proxy_input_idx,
            )?;
        }

        // 10b. Connect terminal outputs: internal source → OutputProxy
        for (node_id, slot_index, proxy_input_idx) in &terminal_slot_map {
            let internal_source = node_id_remap
                .get(node_id)
                .ok_or("Terminal output node not in remap")?;
            internal_graph.connect_nodes(
                internal_source,
                *slot_index,
                &output_proxy_id,
                *proxy_input_idx,
            )?;
        }

        // Release the write lock before modifying the parent graph
        drop(write_subgraph);

        // 11. Rewire parent graph edges to go through SubGraphNode
        // Incoming: external_source → SubGraphNode.input[idx]
        // With source-based dedup, multiple edges from the same source map to the
        // same input port. Deduplicate to avoid creating duplicate parent edges.
        let mut created_parent_inputs: HashSet<(NodeId, usize, usize)> = HashSet::new();
        for boundary in &incoming_edges {
            let input_idx = input_slot_map[&boundary.edge_id];
            self.remove_edge(boundary.edge_id)?;
            let key = (
                boundary.edge.from_node_id,
                boundary.edge.from_output_slot_index,
                input_idx,
            );
            if created_parent_inputs.insert(key) {
                self.connect_nodes(
                    &boundary.edge.from_node_id,
                    boundary.edge.from_output_slot_index,
                    &subgraph_id,
                    input_idx,
                )?;
            }
        }

        // Outgoing: SubGraphNode.output[idx] → external_target
        for boundary in &outgoing_edges {
            let output_idx = output_slot_map[&boundary.edge_id];
            self.remove_edge(boundary.edge_id)?;
            self.connect_nodes(
                &subgraph_id,
                output_idx,
                &boundary.edge.to_node_id,
                boundary.edge.to_input_slot_index,
            )?;
        }

        // 12. Remove internal edges from parent
        for boundary in &internal_edges {
            // These may already be removed if they shared nodes with incoming/outgoing
            let _ = self.remove_edge(boundary.edge_id);
        }

        // 13. Remove dropped boundary edges from parent
        //     These are intermediate edges that were filtered out in step 2.
        for boundary in &dropped_outgoing {
            let _ = self.remove_edge(boundary.edge_id);
        }
        for boundary in &dropped_incoming {
            let _ = self.remove_edge(boundary.edge_id);
        }

        // 14. Remove original nodes from parent graph
        for &node_id in node_ids {
            let _ = self.remove_node(node_id);
        }

        Ok(GroupResult {
            subgraph_node_id: subgraph_id,
            node_id_remap,
        })
    }

    /// Ungroup a SubGraphNode, extracting its internal nodes back into the parent.
    ///
    /// The SubGraphNode is removed and its internal nodes are placed directly
    /// in the parent graph. Edges are rewired to bypass the SubGraphNode.
    pub fn ungroup_node(&mut self, subgraph_node_id: NodeId) -> Result<UngroupResult, String> {
        // 1. Read SubGraphNode's internal state
        let subgraph_entity = self
            .get_node_by_id(&subgraph_node_id)
            .ok_or("SubGraphNode not found")?;
        let read_subgraph = subgraph_entity.read().map_err(|e| e.to_string())?;
        let subgraph = read_subgraph
            .as_any()
            .downcast_ref::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;

        let input_proxy_id = subgraph.input_proxy_id();
        let output_proxy_id = subgraph.output_proxy_id();
        let internal_graph = subgraph.internal_graph();

        // 2. Collect internal nodes (excluding proxies)
        let internal_cache = internal_graph.shared_cache();
        let internal_cache_read = internal_cache.read()?;

        // Get all internal node IDs by examining edges
        let mut internal_node_ids: HashSet<NodeId> = HashSet::new();
        for edge in internal_cache_read.edges.values() {
            if edge.from_node_id != input_proxy_id && edge.from_node_id != output_proxy_id {
                internal_node_ids.insert(edge.from_node_id);
            }
            if edge.to_node_id != input_proxy_id && edge.to_node_id != output_proxy_id {
                internal_node_ids.insert(edge.to_node_id);
            }
        }

        // 3. Collect edge info we need before dropping locks
        // Edges from InputProxy → internal (maps proxy output slot → internal node/slot)
        let mut input_proxy_edges: Vec<(usize, NodeId, usize)> = Vec::new(); // (proxy_out_idx, to_node, to_slot)
        for edge in internal_cache_read.edges.values() {
            if edge.from_node_id == input_proxy_id {
                input_proxy_edges.push((
                    edge.from_output_slot_index,
                    edge.to_node_id,
                    edge.to_input_slot_index,
                ));
            }
        }

        // Edges from internal → OutputProxy (maps internal node/slot → proxy input slot)
        let mut output_proxy_edges: Vec<(NodeId, usize, usize)> = Vec::new(); // (from_node, from_slot, proxy_in_idx)
        for edge in internal_cache_read.edges.values() {
            if edge.to_node_id == output_proxy_id {
                output_proxy_edges.push((
                    edge.from_node_id,
                    edge.from_output_slot_index,
                    edge.to_input_slot_index,
                ));
            }
        }

        // Internal edges (between non-proxy nodes)
        let mut internal_edges: Vec<(NodeId, usize, NodeId, usize)> = Vec::new();
        for edge in internal_cache_read.edges.values() {
            if edge.from_node_id != input_proxy_id
                && edge.to_node_id != output_proxy_id
                && internal_node_ids.contains(&edge.from_node_id)
                && internal_node_ids.contains(&edge.to_node_id)
            {
                internal_edges.push((
                    edge.from_node_id,
                    edge.from_output_slot_index,
                    edge.to_node_id,
                    edge.to_input_slot_index,
                ));
            }
        }

        // Get internal node type names
        let mut node_type_names: HashMap<NodeId, &'static str> = HashMap::new();
        for &node_id in &internal_node_ids {
            if let Some(node) = internal_graph.get_node_by_id(&node_id) {
                if let Ok(read) = node.read() {
                    node_type_names.insert(node_id, read.node_name());
                }
            }
        }

        drop(internal_cache_read);

        // 4. Collect parent graph edges to/from SubGraphNode
        let parent_cache = self.shared_cache();
        let parent_cache_read = parent_cache.read()?;

        // Edges entering SubGraphNode (external → subgraph.input[idx])
        let mut parent_incoming: Vec<(NodeId, usize, usize, EdgeId)> = Vec::new(); // (from_node, from_slot, to_slot_idx, edge_id)
        for (&edge_id, edge) in &parent_cache_read.edges {
            if edge.to_node_id == subgraph_node_id {
                parent_incoming.push((
                    edge.from_node_id,
                    edge.from_output_slot_index,
                    edge.to_input_slot_index,
                    edge_id,
                ));
            }
        }

        // Edges leaving SubGraphNode (subgraph.output[idx] → external)
        let mut parent_outgoing: Vec<(usize, NodeId, usize, EdgeId)> = Vec::new(); // (from_slot_idx, to_node, to_slot, edge_id)
        for (&edge_id, edge) in &parent_cache_read.edges {
            if edge.from_node_id == subgraph_node_id {
                parent_outgoing.push((
                    edge.from_output_slot_index,
                    edge.to_node_id,
                    edge.to_input_slot_index,
                    edge_id,
                ));
            }
        }

        drop(parent_cache_read);
        drop(read_subgraph);

        // 5. Create copies of internal nodes in parent graph
        let mut node_id_remap: HashMap<NodeId, NodeId> = HashMap::new();
        for &internal_id in &internal_node_ids {
            if let Some(&type_name) = node_type_names.get(&internal_id) {
                let parent_id = self.create_node_by_name(type_name)?;
                node_id_remap.insert(internal_id, parent_id);
            }
        }

        // 6. Recreate internal edges in parent
        for (from_id, from_slot, to_id, to_slot) in &internal_edges {
            if let (Some(new_from), Some(new_to)) =
                (node_id_remap.get(from_id), node_id_remap.get(to_id))
            {
                self.connect_nodes(new_from, *from_slot, new_to, *to_slot)?;
            }
        }

        // 7. Rewire incoming edges: external → internal node that was connected to InputProxy
        for (ext_from, ext_from_slot, subgraph_input_idx, edge_id) in &parent_incoming {
            self.remove_edge(*edge_id)?;
            // Find which internal node the InputProxy output[subgraph_input_idx] was connected to
            for (proxy_out_idx, internal_to, internal_to_slot) in &input_proxy_edges {
                if *proxy_out_idx == *subgraph_input_idx {
                    if let Some(new_to) = node_id_remap.get(internal_to) {
                        self.connect_nodes(ext_from, *ext_from_slot, new_to, *internal_to_slot)?;
                    }
                }
            }
        }

        // 8. Rewire outgoing edges: internal node → external
        for (subgraph_output_idx, ext_to, ext_to_slot, edge_id) in &parent_outgoing {
            self.remove_edge(*edge_id)?;
            // Find which internal node was connected to OutputProxy input[subgraph_output_idx]
            for (internal_from, internal_from_slot, proxy_in_idx) in &output_proxy_edges {
                if *proxy_in_idx == *subgraph_output_idx {
                    if let Some(new_from) = node_id_remap.get(internal_from) {
                        self.connect_nodes(new_from, *internal_from_slot, ext_to, *ext_to_slot)?;
                    }
                }
            }
        }

        // 9. Remove the SubGraphNode from parent
        self.remove_node(subgraph_node_id)?;

        let extracted_node_ids: Vec<NodeId> = node_id_remap.values().copied().collect();
        Ok(UngroupResult {
            extracted_node_ids,
        })
    }

    /// Classify edges relative to a set of selected node IDs.
    fn classify_boundary_edges(
        &self,
        selected: &HashSet<NodeId>,
    ) -> Result<(Vec<BoundaryEdge>, Vec<BoundaryEdge>, Vec<BoundaryEdge>), String> {
        let cache = self.shared_cache();
        let cache_read = cache.read()?;

        let mut incoming = Vec::new(); // external → selected
        let mut outgoing = Vec::new(); // selected → external
        let mut internal = Vec::new(); // selected → selected

        for (&edge_id, edge) in &cache_read.edges {
            let from_selected = selected.contains(&edge.from_node_id);
            let to_selected = selected.contains(&edge.to_node_id);

            match (from_selected, to_selected) {
                (false, true) => incoming.push(BoundaryEdge {
                    edge_id,
                    edge: edge.clone(),
                }),
                (true, false) => outgoing.push(BoundaryEdge {
                    edge_id,
                    edge: edge.clone(),
                }),
                (true, true) => internal.push(BoundaryEdge {
                    edge_id,
                    edge: edge.clone(),
                }),
                (false, false) => {} // Not relevant
            }
        }

        Ok((incoming, outgoing, internal))
    }

    /// Get the DataType for an edge by examining its source output slot.
    fn get_edge_data_type(&self, edge: &Edge) -> Result<DataType, String> {
        let node = self
            .get_node_by_id(&edge.from_node_id)
            .ok_or("Edge source node not found")?;
        let read_node = node.read().map_err(|e| e.to_string())?;
        let slot = read_node
            .get_output_slot_by_index(edge.from_output_slot_index)
            .ok_or("Edge source output slot not found")?;
        Ok(slot.data_type)
    }

    /// Group outgoing edges by source slot (from_node_id, from_output_slot_index).
    /// Returns groups of edges that share the same source; one SubGraphNode output
    /// port is created per group.
    fn dedup_outgoing(edges: &[BoundaryEdge]) -> Vec<Vec<&BoundaryEdge>> {
        let mut groups: HashMap<(NodeId, usize), Vec<&BoundaryEdge>> = HashMap::new();
        for edge in edges {
            groups
                .entry((edge.edge.from_node_id, edge.edge.from_output_slot_index))
                .or_default()
                .push(edge);
        }
        groups.into_values().collect()
    }

    /// Group incoming edges by source slot (from_node_id, from_output_slot_index).
    /// Returns groups of edges that share the same source; one SubGraphNode input
    /// port is created per group. The InputProxy fans out to multiple internal targets.
    fn dedup_incoming(edges: &[BoundaryEdge]) -> Vec<Vec<&BoundaryEdge>> {
        let mut groups: HashMap<(NodeId, usize), Vec<&BoundaryEdge>> = HashMap::new();
        for edge in edges {
            groups
                .entry((edge.edge.from_node_id, edge.edge.from_output_slot_index))
                .or_default()
                .push(edge);
        }
        groups.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeGraphAPI;

    #[tokio::test]
    async fn test_group_and_ungroup() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");

        // Create a simple chain: Number → Add → NumberOutput
        let num1 = graph
            .create_node::<crate::NumberNode>()
            .expect("create num1");
        let num2 = graph
            .create_node::<crate::NumberNode>()
            .expect("create num2");
        let add = graph.create_node::<crate::AddNode>().expect("create add");
        let output = graph
            .create_node::<crate::NumberOutput>()
            .expect("create output");

        // Connect: num1 → add.input0, num2 → add.input1, add → output
        graph
            .connect_nodes(&num1, 0, &add, 0)
            .expect("connect num1→add");
        graph
            .connect_nodes(&num2, 0, &add, 1)
            .expect("connect num2→add");
        graph
            .connect_nodes(&add, 0, &output, 0)
            .expect("connect add→output");

        // Group the Add node
        let selected: HashSet<NodeId> = [add].into_iter().collect();
        let result = graph
            .group_nodes(&selected, "TestGroup")
            .expect("group_nodes");

        // Verify SubGraphNode was created
        let sg_entity = graph
            .get_node_by_id(&result.subgraph_node_id)
            .expect("subgraph exists");
        let read_sg = sg_entity.read().expect("read");
        assert_eq!(read_sg.node_name(), "SubGraph");
        // Should have 2 inputs (from num1 and num2) and 1 output (to output)
        assert_eq!(read_sg.inputs().len(), 2);
        assert_eq!(read_sg.outputs().len(), 1);
        drop(read_sg);

        // Verify original add node was removed
        assert!(graph.get_node_by_id(&add).is_none());

        // Ungroup
        let ungroup_result = graph
            .ungroup_node(result.subgraph_node_id)
            .expect("ungroup_node");
        assert_eq!(ungroup_result.extracted_node_ids.len(), 1);

        // Verify SubGraphNode was removed
        assert!(graph.get_node_by_id(&result.subgraph_node_id).is_none());
    }

    #[tokio::test]
    async fn test_group_empty_fails() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");
        let selected: HashSet<NodeId> = HashSet::new();
        assert!(graph.group_nodes(&selected, "Empty").is_err());
    }

    /// Test that intermediate outputs (source slot also feeds internal nodes)
    /// are filtered out and not exposed as SubGraphNode ports.
    #[tokio::test]
    async fn test_group_filters_intermediate_outputs() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");

        // Graph: num1 → add → multiply → output
        //                add → external_consumer (intermediate tap)
        let num1 = graph.create_node::<crate::NumberNode>().unwrap();
        let add = graph.create_node::<crate::AddNode>().unwrap();
        let mul = graph.create_node::<crate::MultiplyNode>().unwrap();
        let output = graph.create_node::<crate::NumberOutput>().unwrap();
        let external = graph.create_node::<crate::NumberOutput>().unwrap();

        graph.connect_nodes(&num1, 0, &add, 0).unwrap();
        graph.connect_nodes(&add, 0, &mul, 0).unwrap();  // internal
        graph.connect_nodes(&add, 0, &external, 0).unwrap(); // intermediate tap
        graph.connect_nodes(&mul, 0, &output, 0).unwrap();

        // Select add + multiply (not num1, output, external)
        let selected: HashSet<NodeId> = [add, mul].into_iter().collect();
        let result = graph.group_nodes(&selected, "FilterTest").unwrap();

        let sg_entity = graph.get_node_by_id(&result.subgraph_node_id).unwrap();
        let read_sg = sg_entity.read().unwrap();

        // 1 input (from num1). Add's second input (slot 1) has no edge → no port.
        assert_eq!(read_sg.inputs().len(), 1, "should have 1 input port");
        // 1 output (mul→output). The add→external edge is intermediate
        // (add's output also feeds mul internally) and should be filtered out.
        assert_eq!(read_sg.outputs().len(), 1, "should have 1 output port (intermediate filtered)");
    }

    /// Test terminal output detection: a node with no downstream consumers
    /// should automatically get an output port on the SubGraphNode.
    #[tokio::test]
    async fn test_group_terminal_output() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");

        // Graph: num1 → add (no downstream from add)
        let num1 = graph.create_node::<crate::NumberNode>().unwrap();
        let add = graph.create_node::<crate::AddNode>().unwrap();

        graph.connect_nodes(&num1, 0, &add, 0).unwrap();

        // Select only the add node
        let selected: HashSet<NodeId> = [add].into_iter().collect();
        let result = graph.group_nodes(&selected, "TerminalTest").unwrap();

        let sg_entity = graph.get_node_by_id(&result.subgraph_node_id).unwrap();
        let read_sg = sg_entity.read().unwrap();

        // 1 input (from num1)
        assert_eq!(read_sg.inputs().len(), 1, "should have 1 input port");
        // 1 output (add.output0 is terminal — no consumers)
        assert_eq!(read_sg.outputs().len(), 1, "should have 1 terminal output port");
    }

    /// Test source-based dedup: same source feeding multiple internal targets
    /// should produce only one input port with fan-out inside the subgraph.
    #[tokio::test]
    async fn test_group_source_dedup() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");

        // Graph: source → A.input0, source → B.input0
        //        (same source output feeds two different internal nodes)
        let source = graph.create_node::<crate::NumberNode>().unwrap();
        let a = graph.create_node::<crate::AddNode>().unwrap();
        let b = graph.create_node::<crate::AddNode>().unwrap();
        let out_a = graph.create_node::<crate::NumberOutput>().unwrap();
        let out_b = graph.create_node::<crate::NumberOutput>().unwrap();

        graph.connect_nodes(&source, 0, &a, 0).unwrap();
        graph.connect_nodes(&source, 0, &b, 0).unwrap();
        graph.connect_nodes(&a, 0, &out_a, 0).unwrap();
        graph.connect_nodes(&b, 0, &out_b, 0).unwrap();

        // Select A and B (not source, not outputs)
        let selected: HashSet<NodeId> = [a, b].into_iter().collect();
        let result = graph.group_nodes(&selected, "DedupTest").unwrap();

        let sg_entity = graph.get_node_by_id(&result.subgraph_node_id).unwrap();
        let read_sg = sg_entity.read().unwrap();

        // With source-based dedup: source.output0 → 1 input port (fans out to A and B)
        assert_eq!(read_sg.inputs().len(), 1, "should have 1 input port (source deduped)");
        // 2 outputs (A→out_a, B→out_b)
        assert_eq!(read_sg.outputs().len(), 2, "should have 2 output ports");
    }
}
