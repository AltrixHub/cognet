//! SubGraph convenience methods for NodeGraph.
//!
//! These methods provide direct access to SubGraphNode operations
//! without requiring callers to perform entity lookup + downcast manually.

use crate::{
    Data, DataType, EdgeId, InterfaceNodeData, NodeGraph, NodeGraphRead, NodeId, NodePath,
    SubGraphNode, INTERFACE_NODE_DATA_DOMAIN,
};

use super::EdgeInfo;

impl NodeGraph {
    // === SubGraph detection & info ===

    /// Check if a node is a SubGraphNode.
    pub fn is_subgraph_node(&self, node_id: &NodeId) -> bool {
        self.get_node_by_id(node_id)
            .and_then(|entity| {
                entity
                    .read()
                    .ok()
                    .map(|guard| guard.as_any().downcast_ref::<SubGraphNode>().is_some())
            })
            .unwrap_or(false)
    }

    /// Get the display label of a SubGraphNode.
    pub fn subgraph_label(&self, node_id: &NodeId) -> Option<String> {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some(sg.label().to_string())
    }

    /// Set the display label of a SubGraphNode.
    ///
    /// Returns true if successful.
    pub fn set_subgraph_label(&self, node_id: &NodeId, label: &str) -> bool {
        let entity = match self.get_node_by_id(node_id) {
            Some(e) => e,
            None => return false,
        };
        let mut guard = match entity.write() {
            Ok(g) => g,
            Err(_) => return false,
        };
        match guard.as_any_mut().downcast_mut::<SubGraphNode>() {
            Some(sg) => {
                sg.set_label(label);
                true
            }
            None => false,
        }
    }

    /// Get the proxy node IDs for a SubGraphNode.
    ///
    /// Returns `(input_proxy_id, output_proxy_id)`.
    pub fn subgraph_proxy_ids(&self, node_id: &NodeId) -> Option<(NodeId, NodeId)> {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some((sg.input_proxy_id(), sg.output_proxy_id()))
    }

    // === SubGraph internal access ===

    /// Get all node IDs in a SubGraphNode's internal graph.
    pub fn subgraph_node_ids(&self, node_id: &NodeId) -> Option<Vec<NodeId>> {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        let ns = sg.internal_graph().node_states().read().ok()?;
        Some(ns.node_ids().collect())
    }

    /// Get all edges in a SubGraphNode's internal graph.
    pub fn subgraph_edges(&self, node_id: &NodeId) -> Vec<EdgeInfo> {
        let info = (|| -> Option<Vec<EdgeInfo>> {
            let entity = self.get_node_by_id(node_id)?;
            let guard = entity.read().ok()?;
            let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
            let ns = sg.internal_graph().node_states().read().ok()?;
            Some(
                ns.edges()
                    .iter()
                    .map(|(id, edge)| EdgeInfo {
                        id: *id,
                        from_node: edge.from_node_id,
                        from_output: edge.from_output_slot_index,
                        to_node: edge.to_node_id,
                        to_input: edge.to_input_slot_index,
                    })
                    .collect(),
            )
        })();
        info.unwrap_or_default()
    }

    /// Execute a closure with read access to a SubGraphNode's internal graph.
    pub fn with_subgraph<F, R>(&self, node_id: &NodeId, f: F) -> Option<R>
    where
        F: FnOnce(&NodeGraph) -> R,
    {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some(f(sg.internal_graph()))
    }

    /// Execute a closure with mutable access to a SubGraphNode's internal graph.
    ///
    /// Note: Takes `&self` because mutation happens inside the SubGraphNode entity
    /// (which has its own RwLock), not on the outer NodeGraph.
    pub fn with_subgraph_mut<F, R>(&self, node_id: &NodeId, f: F) -> Option<R>
    where
        F: FnOnce(&mut NodeGraph) -> R,
    {
        let entity = self.get_node_by_id(node_id)?;
        let mut guard = entity.write().ok()?;
        let sg = guard.as_any_mut().downcast_mut::<SubGraphNode>()?;
        Some(f(sg.internal_graph_mut()))
    }

    // === SubGraph port management ===

    /// Add an input port to a SubGraphNode.
    pub fn add_subgraph_input(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.add_input(label, data_type)?;
        drop(guard);
        // Sync parent NodeStates
        if let Ok(mut ns) = self.node_states.write() {
            ns.add_input_slot(&NodePath::root().child(*node_id), label, data_type, None);
        }
        Ok(())
    }

    /// Remove an input port from a SubGraphNode by index.
    pub fn remove_subgraph_input(&self, node_id: &NodeId, index: usize) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.remove_input(index)?;
        drop(guard);
        // Sync parent NodeStates
        if let Ok(mut ns) = self.node_states.write() {
            ns.remove_input_slot(&NodePath::root().child(*node_id), index);
        }
        Ok(())
    }

    /// Add an output port to a SubGraphNode.
    pub fn add_subgraph_output(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.add_output(label, data_type)?;
        drop(guard);
        // Sync parent NodeStates
        if let Ok(mut ns) = self.node_states.write() {
            ns.add_output_slot(&NodePath::root().child(*node_id), label, data_type);
        }
        Ok(())
    }

    /// Remove an output port from a SubGraphNode by index.
    pub fn remove_subgraph_output(&self, node_id: &NodeId, index: usize) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.remove_output(index)?;
        drop(guard);
        // Sync parent NodeStates
        if let Ok(mut ns) = self.node_states.write() {
            ns.remove_output_slot(&NodePath::root().child(*node_id), index);
        }
        Ok(())
    }

    /// Add a **locked** input port to a SubGraphNode.
    ///
    /// Same effect as [`Self::add_subgraph_input`], plus appends
    /// `label` to the inner Input-direction `InterfaceNode`'s
    /// `NodeData.locked` set so subsequent
    /// `remove_subgraph_input_by_label` / `rename_subgraph_input` are
    /// rejected for this label.
    ///
    /// Plan-005 Task 5 — used by variant builders (e.g. Wall) to mark
    /// the mandatory ports `base_polyline` / `currency` / `fx_rate` at
    /// SubGraph construction time.
    pub fn add_subgraph_input_locked(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        self.add_subgraph_input(node_id, label, data_type)?;
        let proxies = self
            .subgraph_proxy_ids(node_id)
            .ok_or("Node is not a SubGraphNode")?;
        self.with_subgraph_mut(node_id, |internal| {
            append_locked_label(internal, &proxies.0, label)
        })
        .ok_or_else(|| "with_subgraph_mut returned None".to_string())?
    }

    /// Add a **locked** output port to a SubGraphNode. Mirror of
    /// [`Self::add_subgraph_input_locked`] targeting the inner
    /// Output-direction `InterfaceNode`.
    pub fn add_subgraph_output_locked(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        self.add_subgraph_output(node_id, label, data_type)?;
        let proxies = self
            .subgraph_proxy_ids(node_id)
            .ok_or("Node is not a SubGraphNode")?;
        self.with_subgraph_mut(node_id, |internal| {
            append_locked_label(internal, &proxies.1, label)
        })
        .ok_or_else(|| "with_subgraph_mut returned None".to_string())?
    }

    /// Remove an input port from a SubGraphNode by label.
    ///
    /// Drops every edge that touched the removed slot index and
    /// shifts indices DOWN by 1 for all subsequent input slots on
    /// this node (both inside the proxy and on the parent NodeStates).
    pub fn remove_subgraph_input_by_label(
        &self,
        node_id: &NodeId,
        label: &str,
    ) -> Result<usize, String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        let index = sg
            .input_slot_index_by_label(label)
            .ok_or_else(|| format!("Input label {:?} not found", label))?;
        sg.remove_input(index)?;
        drop(guard);

        // Sync parent NodeStates: drop incoming edges touching this slot,
        // then remove + shift.
        if let Ok(mut ns) = self.node_states.write() {
            let edges_to_remove: Vec<EdgeId> = ns
                .edges()
                .iter()
                .filter(|(_, edge)| {
                    edge.to_node_id == *node_id && edge.to_input_slot_index == index
                })
                .map(|(eid, _)| *eid)
                .collect();
            for eid in edges_to_remove {
                ns.remove_edge(&eid);
            }
            ns.remove_input_slot(&NodePath::root().child(*node_id), index);
        }
        Ok(index)
    }

    /// Remove an output port from a SubGraphNode by label.
    ///
    /// Drops every edge that touched the removed slot (both the parent
    /// edge on the external output and any internal edges feeding the
    /// proxy's input slot or consuming the mirror output) and shifts
    /// indices DOWN by 1 for all subsequent output slots.
    pub fn remove_subgraph_output_by_label(
        &self,
        node_id: &NodeId,
        label: &str,
    ) -> Result<usize, String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        let index = sg
            .output_slot_index_by_label(label)
            .ok_or_else(|| format!("Output label {:?} not found", label))?;
        sg.remove_output(index)?;
        drop(guard);

        // Sync parent NodeStates: drop outgoing edges touching this slot,
        // then remove + shift.
        if let Ok(mut ns) = self.node_states.write() {
            let edges_to_remove: Vec<EdgeId> = ns
                .edges()
                .iter()
                .filter(|(_, edge)| {
                    edge.from_node_id == *node_id && edge.from_output_slot_index == index
                })
                .map(|(eid, _)| *eid)
                .collect();
            for eid in edges_to_remove {
                ns.remove_edge(&eid);
            }
            ns.remove_output_slot(&NodePath::root().child(*node_id), index);
        }
        Ok(index)
    }

    /// Rename an input port of a SubGraphNode in place.
    ///
    /// Slot index and slot id are preserved so all edges remain valid.
    /// Returns `Err` if `old_label` is not found or `new_label` is
    /// already in use on the same side.
    pub fn rename_subgraph_input(
        &self,
        node_id: &NodeId,
        old_label: &str,
        new_label: &'static str,
    ) -> Result<usize, String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        let index = sg.rename_input(old_label, new_label)?;
        drop(guard);

        // Sync parent NodeStates label.
        if let Ok(mut ns) = self.node_states.write() {
            ns.set_input_slot_label(&NodePath::root().child(*node_id), index, new_label);
        }
        Ok(index)
    }

    /// Rename an output port of a SubGraphNode in place. The mirror
    /// output slot is renamed too.
    ///
    /// Slot index and slot id are preserved so all edges remain valid.
    /// Returns `Err` if `old_label` is not found or `new_label` is
    /// already in use on the same side.
    pub fn rename_subgraph_output(
        &self,
        node_id: &NodeId,
        old_label: &str,
        new_label: &'static str,
    ) -> Result<usize, String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        let index = sg.rename_output(old_label, new_label)?;
        drop(guard);

        // Sync parent NodeStates label.
        if let Ok(mut ns) = self.node_states.write() {
            ns.set_output_slot_label(&NodePath::root().child(*node_id), index, new_label);
        }
        Ok(index)
    }

    /// Add a multi-input port to a SubGraphNode.
    ///
    /// Creates one external input port that maps to multiple proxy outputs inside
    /// the internal graph. Used for ports like "Baseline" where N edges connect to
    /// one input slot and each edge value is distributed to a separate proxy output.
    pub fn add_subgraph_multi_input(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
        proxy_count: usize,
        proxy_data_type: DataType,
        proxy_label: &'static str,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.add_multi_input(proxy_count, proxy_data_type, proxy_label)?;
        drop(guard);
        // Sync parent NodeStates
        if let Ok(mut ns) = self.node_states.write() {
            ns.add_input_slot(&NodePath::root().child(*node_id), label, data_type, None);
        }
        Ok(())
    }

    /// Add only the external (parent NodeStates) input port for a SubGraphNode.
    ///
    /// Used when the internal proxy setup is handled separately (e.g., dynamic inputs).
    pub fn add_subgraph_input_external_only(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        if let Ok(mut ns) = self.node_states.write() {
            ns.add_input_slot(&NodePath::root().child(*node_id), label, data_type, None);
        }
        Ok(())
    }

    /// Add a dynamic multi-input port to a SubGraphNode.
    ///
    /// Unlike `add_subgraph_multi_input` (fixed proxy count), a dynamic input
    /// starts with zero proxy outputs and automatically grows/shrinks at
    /// execution time to match the number of connected edges.
    ///
    /// `target_node_id` and `target_slot` identify which internal node's
    /// multi-connection input slot the proxy outputs should connect to.
    #[allow(clippy::too_many_arguments)]
    pub fn add_subgraph_dynamic_input(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
        target_node_id: NodeId,
        target_slot: usize,
        proxy_data_type: DataType,
        proxy_label: &'static str,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.add_dynamic_input(target_node_id, target_slot, proxy_data_type, proxy_label);
        drop(guard);
        // Sync parent NodeStates
        if let Ok(mut ns) = self.node_states.write() {
            ns.add_input_slot(&NodePath::root().child(*node_id), label, data_type, None);
        }
        Ok(())
    }

    /// Set the default value for a SubGraphNode input port.
    pub fn set_subgraph_input_default(
        &self,
        node_id: &NodeId,
        input_index: usize,
        data: Data,
    ) -> Result<(), String> {
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        let slot = ns
            .input_slot_mut(&NodePath::root().child(*node_id), input_index)
            .ok_or_else(|| {
                format!(
                    "Input slot {} not found for node {:?}",
                    input_index, node_id
                )
            })?;
        slot.default_value = Some(data.into_value());
        ns.mark_changed(&NodePath::root().child(*node_id));
        Ok(())
    }
}

/// Append `label` to the `NodeData.locked` set of an `InterfaceNode`.
///
/// Used by [`NodeGraph::add_subgraph_input_locked`] /
/// [`NodeGraph::add_subgraph_output_locked`] to mark a freshly-added
/// port as mandatory. If the node has no NodeData yet, a fresh
/// [`InterfaceNodeData`] is inserted with this label as the only
/// locked entry. If the node has NodeData of a wrong domain, returns
/// `Err` (the slot was already added — caller may want to roll back).
fn append_locked_label(
    graph: &NodeGraph,
    node_id: &NodeId,
    label: &'static str,
) -> Result<(), String> {
    let existing = graph.node_data(node_id);
    let mut data = match existing {
        None => InterfaceNodeData::default(),
        Some(d) => match d.get_type() {
            DataType::Domain(name) if name == INTERFACE_NODE_DATA_DOMAIN => d
                .value::<InterfaceNodeData>()
                .map_err(|e| format!("InterfaceNodeData downcast: {e}"))?
                .clone(),
            other => {
                return Err(format!(
                    "InterfaceNode {:?} has non-matching NodeData domain {:?}; expected Domain(\"{}\")",
                    node_id, other, INTERFACE_NODE_DATA_DOMAIN
                ));
            }
        },
    };
    data.locked.insert(label.to_string());
    let mut ns = graph
        .node_states
        .write()
        .map_err(|e| format!("NodeStates lock poisoned: {e}"))?;
    let state = ns
        .get_mut(&NodePath::root().child(*node_id))
        .ok_or_else(|| format!("Node {:?} disappeared mid-call", node_id))?;
    state.data = Some(Data::from_domain(data, INTERFACE_NODE_DATA_DOMAIN));
    ns.mark_changed(&NodePath::root().child(*node_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{Data, DataType, NodeGraphWrite, SubGraphNode};

    #[test]
    fn test_is_subgraph_node() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let num = graph
            .create_node::<crate::NumberNode>()
            .expect("create num");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        assert!(!graph.is_subgraph_node(&num));
        assert!(graph.is_subgraph_node(&sg));
    }

    #[test]
    fn test_subgraph_label() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        assert_eq!(graph.subgraph_label(&sg), Some("SubGraph".to_string()));
        assert!(graph.set_subgraph_label(&sg, "Custom"));
        assert_eq!(graph.subgraph_label(&sg), Some("Custom".to_string()));
    }

    #[test]
    fn test_subgraph_proxy_ids() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        let (input_id, output_id) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        // Proxy IDs should be valid NodeIds in the internal graph
        let internal_ids = graph.subgraph_node_ids(&sg).expect("internal ids");
        assert!(internal_ids.contains(&input_id));
        assert!(internal_ids.contains(&output_id));
    }

    #[test]
    fn test_add_remove_subgraph_ports() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Add input
        graph
            .add_subgraph_input(&sg, "In1", crate::DataType::Number)
            .expect("add input");

        // Add output
        graph
            .add_subgraph_output(&sg, "Out1", crate::DataType::Number)
            .expect("add output");

        // Verify via NodeStates
        {
            let ns = graph.node_states().read().unwrap();
            assert_eq!(ns.input_slot_count(&crate::NodePath::root().child(sg)), 1);
            assert_eq!(ns.output_slot_count(&crate::NodePath::root().child(sg)), 1);
        }

        // Remove
        graph.remove_subgraph_input(&sg, 0).expect("remove input");
        graph.remove_subgraph_output(&sg, 0).expect("remove output");

        {
            let ns = graph.node_states().read().unwrap();
            assert_eq!(ns.input_slot_count(&crate::NodePath::root().child(sg)), 0);
            assert_eq!(ns.output_slot_count(&crate::NodePath::root().child(sg)), 0);
        }
    }

    #[test]
    fn test_with_subgraph() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Read access
        let count = graph
            .with_subgraph(&sg, |internal| internal.node_ids().len())
            .expect("with_subgraph");
        assert_eq!(count, 2); // input + output proxies

        // Mut access: create a node inside
        let internal_id = graph
            .with_subgraph_mut(&sg, |internal| internal.create_node_by_name("Number"))
            .expect("with_subgraph_mut")
            .expect("create internal node");

        let ids = graph.subgraph_node_ids(&sg).expect("subgraph node ids");
        assert!(ids.contains(&internal_id));
    }

    /// Verify that the SubGraphOutputNode proxy emits the same value on
    /// its mirror output slot as on the parent SubGraphNode's external
    /// output slot.
    #[test]
    fn test_subgraph_output_proxy_mirrors_value() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<SubGraphNode>()
            .expect("create subgraph");

        graph
            .add_subgraph_output(&sg, "test_out", DataType::Number)
            .expect("add output");

        let (_in_proxy, out_proxy) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        let inner_const = graph
            .with_subgraph_mut(&sg, |internal| {
                let n = internal.create_node::<crate::NumberNode>().unwrap();
                internal
                    .update_node_data(&n, Data::new(13.0_f64).unwrap())
                    .unwrap();
                internal.connect_nodes(&n, 0, &out_proxy, 0).unwrap();
                n
            })
            .unwrap();

        let result = graph.execute_sync().expect("execute");

        // External output slot of the parent SubGraphNode emits 13.0
        let parent_out = result
            .node_outputs
            .get(&sg)
            .and_then(|slots| slots.first().cloned().flatten())
            .and_then(|d| d.value::<f64>().ok().copied())
            .expect("parent external output present");
        assert_eq!(parent_out, 13.0);

        // Mirror output slot on the internal output proxy emits 13.0
        let mirror = graph
            .with_subgraph(&sg, |internal| {
                let ns = internal.node_states().read().unwrap();
                let slot = ns
                    .output_slot(&crate::NodePath::root().child(out_proxy), 0)
                    .expect("mirror output slot exists");
                let slot_id = slot.id;
                drop(ns);
                let cache = internal.shared_cache();
                let cache_read = cache.read().unwrap();
                cache_read
                    .outputs
                    .get(&slot_id)
                    .and_then(|d| d.value::<f64>().ok().copied())
            })
            .flatten()
            .expect("mirror output slot value present");
        assert_eq!(mirror, 13.0);

        let _ = inner_const; // used for clarity
    }

    /// Verify that removing a SubGraphNode output by label drops every
    /// edge that touched the removed slot, removes the matching mirror
    /// output, and shifts subsequent slot indices down by 1.
    #[test]
    fn test_remove_subgraph_output_by_label_shifts_indices_and_drops_edges() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<SubGraphNode>()
            .expect("create subgraph");

        graph
            .add_subgraph_output(&sg, "a", DataType::Number)
            .expect("add a");
        graph
            .add_subgraph_output(&sg, "b", DataType::Number)
            .expect("add b");
        graph
            .add_subgraph_output(&sg, "c", DataType::Number)
            .expect("add c");

        let (_in_proxy, out_proxy) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");

        // Wire an internal Number → mirror output "b" so we can verify
        // the edge is dropped on remove.
        let (inner_b, inner_c, edge_b_mirror, edge_c_input) = graph
            .with_subgraph_mut(&sg, |internal| {
                let inner_b = internal.create_node::<crate::NumberNode>().unwrap();
                let inner_c = internal.create_node::<crate::NumberNode>().unwrap();
                // Reader nodes that observe the mirror outputs and the
                // OutputProxy input edges respectively.
                let reader_on_mirror_b = internal.create_node::<crate::NumberOutput>().unwrap();
                // Use a Number output sink as a reader.
                let _ = reader_on_mirror_b;
                // Internal Number → OutputProxy.input[1] ("b")
                let _e1 = internal.connect_nodes(&inner_b, 0, &out_proxy, 1).unwrap();
                // Internal Number → OutputProxy.input[2] ("c")
                let e2 = internal.connect_nodes(&inner_c, 0, &out_proxy, 2).unwrap();
                // OutputProxy mirror output[1] ("b") → reader_on_mirror_b
                let edge_b_mirror = internal
                    .connect_nodes(&out_proxy, 1, &reader_on_mirror_b, 0)
                    .unwrap();
                (inner_b, inner_c, edge_b_mirror, e2)
            })
            .unwrap();

        // Sanity: 3 output slots on parent, 3 input slots + 3 output
        // slots on the output proxy.
        {
            let ns = graph.node_states().read().unwrap();
            assert_eq!(ns.output_slot_count(&crate::NodePath::root().child(sg)), 3);
        }
        graph
            .with_subgraph(&sg, |internal| {
                let ns = internal.node_states().read().unwrap();
                assert_eq!(
                    ns.input_slot_count(&crate::NodePath::root().child(out_proxy)),
                    3
                );
                assert_eq!(
                    ns.output_slot_count(&crate::NodePath::root().child(out_proxy)),
                    3
                );
                assert!(ns.get_edge(&edge_b_mirror).is_some());
                assert!(ns.get_edge(&edge_c_input).is_some());
            })
            .unwrap();

        // Remove "b" by label.
        let removed_idx = graph
            .remove_subgraph_output_by_label(&sg, "b")
            .expect("remove by label");
        assert_eq!(removed_idx, 1);

        // Parent now has slots [a, c] at indices 0 and 1.
        {
            let ns = graph.node_states().read().unwrap();
            assert_eq!(ns.output_slot_count(&crate::NodePath::root().child(sg)), 2);
            let s0 = ns
                .output_slot(&crate::NodePath::root().child(sg), 0)
                .unwrap();
            let s1 = ns
                .output_slot(&crate::NodePath::root().child(sg), 1)
                .unwrap();
            assert_eq!(s0.label, "a");
            assert_eq!(s1.label, "c");
        }

        // The "b" mirror edge is gone; the "c" input edge survived with
        // its slot_index decremented (was 2, now 1).
        graph
            .with_subgraph(&sg, |internal| {
                let ns = internal.node_states().read().unwrap();
                assert_eq!(
                    ns.input_slot_count(&crate::NodePath::root().child(out_proxy)),
                    2
                );
                assert_eq!(
                    ns.output_slot_count(&crate::NodePath::root().child(out_proxy)),
                    2
                );
                assert!(
                    ns.get_edge(&edge_b_mirror).is_none(),
                    "edge to mirror output 'b' should be dropped"
                );
                let surviving = ns
                    .get_edge(&edge_c_input)
                    .expect("c-input edge should survive");
                assert_eq!(
                    surviving.to_input_slot_index, 1,
                    "c-input edge should now reference slot_index 1 (was 2)"
                );
            })
            .unwrap();

        let _ = inner_b;
        let _ = inner_c;
    }

    /// Verify that renaming a SubGraphNode input by label keeps the
    /// slot index unchanged and the underlying edges intact.
    #[test]
    fn test_rename_subgraph_input_preserves_edges() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<SubGraphNode>()
            .expect("create subgraph");

        graph
            .add_subgraph_input(&sg, "height", DataType::Number)
            .expect("add height");
        graph
            .add_subgraph_input(&sg, "thickness", DataType::Number)
            .expect("add thickness");

        let outer_h = graph
            .create_node::<crate::NumberNode>()
            .expect("create outer h");
        let edge_into_height = graph
            .connect_nodes(&outer_h, 0, &sg, 0)
            .expect("connect into height");

        // Rename "height" → "wall_height". Slot index should stay 0.
        let idx = graph
            .rename_subgraph_input(&sg, "height", "wall_height")
            .expect("rename ok");
        assert_eq!(idx, 0);

        {
            let ns = graph.node_states().read().unwrap();
            assert_eq!(ns.input_slot_count(&crate::NodePath::root().child(sg)), 2);
            let s0 = ns
                .input_slot(&crate::NodePath::root().child(sg), 0)
                .unwrap();
            let s1 = ns
                .input_slot(&crate::NodePath::root().child(sg), 1)
                .unwrap();
            assert_eq!(s0.label, "wall_height");
            assert_eq!(s1.label, "thickness");
            // The edge survives.
            assert!(ns.get_edge(&edge_into_height).is_some());
        }

        // Collision: rename "thickness" → "wall_height" fails.
        let err = graph
            .rename_subgraph_input(&sg, "thickness", "wall_height")
            .expect_err("collision");
        assert!(err.contains("uniqueness"), "unexpected error: {err}");
    }

    /// Verify that renaming a SubGraphNode output renames both the
    /// proxy input slot AND the mirror output slot.
    #[test]
    fn test_rename_subgraph_output_renames_mirror() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<SubGraphNode>()
            .expect("create subgraph");

        graph
            .add_subgraph_output(&sg, "old_name", DataType::Number)
            .expect("add output");

        let idx = graph
            .rename_subgraph_output(&sg, "old_name", "new_name")
            .expect("rename ok");
        assert_eq!(idx, 0);

        // Parent external output slot is renamed.
        {
            let ns = graph.node_states().read().unwrap();
            let slot = ns
                .output_slot(&crate::NodePath::root().child(sg), 0)
                .unwrap();
            assert_eq!(slot.label, "new_name");
        }

        // Proxy input slot AND mirror output slot are renamed.
        let (_in_proxy, out_proxy) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        graph
            .with_subgraph(&sg, |internal| {
                let ns = internal.node_states().read().unwrap();
                let in_slot = ns
                    .input_slot(&crate::NodePath::root().child(out_proxy), 0)
                    .unwrap();
                let mirror = ns
                    .output_slot(&crate::NodePath::root().child(out_proxy), 0)
                    .unwrap();
                assert_eq!(in_slot.label, "new_name");
                assert_eq!(mirror.label, "new_name");
            })
            .unwrap();
    }
}
