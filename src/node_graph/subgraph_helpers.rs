//! SubGraph convenience methods for NodeGraph.
//!
//! After the transparent-container rework, SubGraph children live in the
//! parent `NodeStates` at paths `sg_path.child(child_id)`. There is no
//! `internal_graph` field or `with_subgraph` / `with_subgraph_mut` pattern.
//! All methods operate on the unified parent `NodeStates` and `NodeManager`.

use std::sync::Arc;

use crate::{
    Data, DataType, EdgeId, InterfaceNodeData, NodeGraph, NodeId, NodePath, SubGraphNode,
    INTERFACE_NODE_DATA_DOMAIN,
};

use super::EdgeInfo;

impl NodeGraph {
    // === SubGraph detection & info ===

    /// Check if a node is a SubGraphNode.
    pub fn is_subgraph_node(&self, node_id: &NodeId) -> bool {
        self.node_manager
            .get_at(&NodePath::root().child(*node_id))
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
        let entity = self
            .node_manager
            .get_at(&NodePath::root().child(*node_id))?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some(sg.label().to_string())
    }

    /// Set the display label of a SubGraphNode.
    pub fn set_subgraph_label(&self, node_id: &NodeId, label: &str) -> bool {
        let entity = match self.node_manager.get_at(&NodePath::root().child(*node_id)) {
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

    /// Get the `(input_proxy_id, output_proxy_id)` pair for a root-level SubGraphNode.
    ///
    /// For path-aware lookup, use `subgraph_proxy_ids_at_path`.
    pub fn subgraph_proxy_ids(&self, node_id: &NodeId) -> Option<(NodeId, NodeId)> {
        let entity = self
            .node_manager
            .get_at(&NodePath::root().child(*node_id))?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some((sg.input_proxy_id(), sg.output_proxy_id()))
    }

    // === SubGraph child node enumeration ===

    /// Get all NodeIds of the direct children of this SubGraphNode.
    ///
    /// In the transparent-container architecture, children
    /// live in the parent `NodeStates` at `sg_path.child(child_id)`.
    pub fn subgraph_node_ids(&self, node_id: &NodeId) -> Option<Vec<NodeId>> {
        let sg_path = NodePath::root().child(*node_id);
        let ns = self.node_states.read().ok()?;
        Some(ns.path_index_children_of(&sg_path).to_vec())
    }

    /// Get all edges between nodes that are direct children of a SubGraphNode.
    ///
    /// Returns edges whose both endpoints are direct children of `node_id`'s
    /// SubGraph (i.e. both `from_node` and `to_node` are `sg_path.child(_)`).
    pub fn subgraph_edges(&self, node_id: &NodeId) -> Vec<EdgeInfo> {
        let sg_path = NodePath::root().child(*node_id);
        let ns = match self.node_states.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let children: std::collections::HashSet<NodeId> = ns
            .path_index_children_of(&sg_path)
            .iter()
            .copied()
            .collect();
        ns.edges()
            .iter()
            .filter(|(_, edge)| {
                // Both endpoints must be direct children of the SubGraph.
                edge.from_node
                    .leaf()
                    .map(|id| children.contains(&id))
                    .unwrap_or(false)
                    && edge
                        .to_node
                        .leaf()
                        .map(|id| children.contains(&id))
                        .unwrap_or(false)
            })
            .map(|(id, edge)| EdgeInfo {
                id: *id,
                from_node: edge.from_node.clone(),
                from_output: edge.from_output_slot_index,
                to_node: edge.to_node.clone(),
                to_input: edge.to_input_slot_index,
            })
            .collect()
    }

    // === SubGraph port management ===

    /// Add an input port to a SubGraphNode.
    ///
    /// Writes one input slot + one output slot on the input-proxy
    /// InterfaceNode at `sg_path.child(input_proxy_id)`.
    pub fn add_subgraph_input(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        let (input_proxy_id, _) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let in_path = sg_path.child(input_proxy_id);
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.add_input_slot(&in_path, label, data_type, Some(1));
        ns.add_output_slot(&in_path, label, data_type);
        // Sync external slot on parent SubGraphNode.
        ns.add_input_slot(&NodePath::root().child(*node_id), label, data_type, None);
        Ok(())
    }

    /// Remove an input port from a SubGraphNode by index.
    pub fn remove_subgraph_input(&self, node_id: &NodeId, index: usize) -> Result<(), String> {
        let (input_proxy_id, _) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let in_path = sg_path.child(input_proxy_id);

        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        // Remove from proxy.
        let mut proxy_edges_to_remove: Vec<EdgeId> = Vec::new();
        if let Some(slot) = ns.input_slot(&in_path, index) {
            let slot_id = slot.id;
            for (eid, edge) in ns.edges() {
                if edge.to_input_slot_id == slot_id {
                    proxy_edges_to_remove.push(*eid);
                }
            }
        }
        if let Some(slot) = ns.output_slot(&in_path, index) {
            let slot_id = slot.id;
            for (eid, edge) in ns.edges() {
                if edge.from_output_slot_id == slot_id {
                    proxy_edges_to_remove.push(*eid);
                }
            }
        }
        for eid in proxy_edges_to_remove {
            ns.remove_edge(&eid);
        }
        ns.remove_input_slot(&in_path, index);
        ns.remove_output_slot(&in_path, index);
        // Sync external slot on parent SubGraphNode.
        let parent_path = NodePath::root().child(*node_id);
        let mut parent_edges_to_remove: Vec<EdgeId> = Vec::new();
        if let Some(slot) = ns.input_slot(&parent_path, index) {
            let slot_id = slot.id;
            for (eid, edge) in ns.edges() {
                if edge.to_input_slot_id == slot_id {
                    parent_edges_to_remove.push(*eid);
                }
            }
        }
        for eid in parent_edges_to_remove {
            ns.remove_edge(&eid);
        }
        ns.remove_input_slot(&parent_path, index);
        Ok(())
    }

    /// Add an output port to a SubGraphNode.
    ///
    /// Writes one input slot + one output slot on the output-proxy
    /// InterfaceNode at `sg_path.child(output_proxy_id)`.
    pub fn add_subgraph_output(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        let (_, output_proxy_id) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let out_path = sg_path.child(output_proxy_id);
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.add_input_slot(&out_path, label, data_type, None);
        ns.add_output_slot(&out_path, label, data_type);
        // Sync external slot on parent SubGraphNode.
        ns.add_output_slot(&NodePath::root().child(*node_id), label, data_type);
        Ok(())
    }

    /// Remove an output port from a SubGraphNode by index.
    pub fn remove_subgraph_output(&self, node_id: &NodeId, index: usize) -> Result<(), String> {
        let (_, output_proxy_id) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let out_path = sg_path.child(output_proxy_id);

        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        let mut proxy_edges_to_remove: Vec<EdgeId> = Vec::new();
        if let Some(slot) = ns.input_slot(&out_path, index) {
            let slot_id = slot.id;
            for (eid, edge) in ns.edges() {
                if edge.to_input_slot_id == slot_id {
                    proxy_edges_to_remove.push(*eid);
                }
            }
        }
        if let Some(slot) = ns.output_slot(&out_path, index) {
            let slot_id = slot.id;
            for (eid, edge) in ns.edges() {
                if edge.from_output_slot_id == slot_id {
                    proxy_edges_to_remove.push(*eid);
                }
            }
        }
        for eid in proxy_edges_to_remove {
            ns.remove_edge(&eid);
        }
        ns.remove_input_slot(&out_path, index);
        ns.remove_output_slot(&out_path, index);
        // Sync external slot on parent SubGraphNode.
        let parent_path = NodePath::root().child(*node_id);
        let mut parent_edges_to_remove: Vec<EdgeId> = Vec::new();
        if let Some(slot) = ns.output_slot(&parent_path, index) {
            let slot_id = slot.id;
            for (eid, edge) in ns.edges() {
                if edge.from_output_slot_id == slot_id {
                    parent_edges_to_remove.push(*eid);
                }
            }
        }
        for eid in parent_edges_to_remove {
            ns.remove_edge(&eid);
        }
        ns.remove_output_slot(&parent_path, index);
        Ok(())
    }

    /// Add a **locked** input port to a SubGraphNode.
    pub fn add_subgraph_input_locked(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        self.add_subgraph_input(node_id, label, data_type)?;
        let (input_proxy_id, _) = self
            .subgraph_proxy_ids(node_id)
            .ok_or("Node is not a SubGraphNode")?;
        let sg_path = NodePath::root().child(*node_id);
        let in_path = sg_path.child(input_proxy_id);
        append_locked_label_at(self, &in_path, label)?;
        Ok(())
    }

    /// Add a **locked** output port to a SubGraphNode.
    pub fn add_subgraph_output_locked(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        self.add_subgraph_output(node_id, label, data_type)?;
        let (_, output_proxy_id) = self
            .subgraph_proxy_ids(node_id)
            .ok_or("Node is not a SubGraphNode")?;
        let sg_path = NodePath::root().child(*node_id);
        let out_path = sg_path.child(output_proxy_id);
        append_locked_label_at(self, &out_path, label)?;
        Ok(())
    }

    /// Remove an input port from a SubGraphNode by label.
    pub fn remove_subgraph_input_by_label(
        &self,
        node_id: &NodeId,
        label: &str,
    ) -> Result<usize, String> {
        let (input_proxy_id, _) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let in_path = sg_path.child(input_proxy_id);

        // Find slot index by label on the proxy.
        let index = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let count = ns.input_slot_count(&in_path);
            (0..count)
                .find(|i| ns.input_slot(&in_path, *i).map(|s| s.label) == Some(label))
                .ok_or_else(|| format!("Input label {:?} not found", label))?
        };

        self.remove_subgraph_input(node_id, index)?;
        Ok(index)
    }

    /// Remove an output port from a SubGraphNode by label.
    pub fn remove_subgraph_output_by_label(
        &self,
        node_id: &NodeId,
        label: &str,
    ) -> Result<usize, String> {
        let (_, output_proxy_id) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let out_path = sg_path.child(output_proxy_id);

        let index = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let count = ns.input_slot_count(&out_path);
            (0..count)
                .find(|i| ns.input_slot(&out_path, *i).map(|s| s.label) == Some(label))
                .ok_or_else(|| format!("Output label {:?} not found", label))?
        };

        self.remove_subgraph_output(node_id, index)?;
        Ok(index)
    }

    /// Rename an input port of a SubGraphNode in place.
    pub fn rename_subgraph_input(
        &self,
        node_id: &NodeId,
        old_label: &str,
        new_label: &'static str,
    ) -> Result<usize, String> {
        let (input_proxy_id, _) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let in_path = sg_path.child(input_proxy_id);

        let index = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let count = ns.input_slot_count(&in_path);
            // Check uniqueness.
            if (0..count).any(|i| ns.input_slot(&in_path, i).map(|s| s.label) == Some(new_label))
                && old_label != new_label
            {
                return Err(format!(
                    "Input label {:?} already exists (uniqueness)",
                    new_label
                ));
            }
            (0..count)
                .find(|i| ns.input_slot(&in_path, *i).map(|s| s.label) == Some(old_label))
                .ok_or_else(|| format!("Input label {:?} not found", old_label))?
        };

        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.set_input_slot_label(&in_path, index, new_label);
        ns.set_output_slot_label(&in_path, index, new_label);
        // Rename external slot on parent.
        let parent_path = NodePath::root().child(*node_id);
        ns.set_input_slot_label(&parent_path, index, new_label);
        Ok(index)
    }

    /// Rename an output port of a SubGraphNode in place.
    pub fn rename_subgraph_output(
        &self,
        node_id: &NodeId,
        old_label: &str,
        new_label: &'static str,
    ) -> Result<usize, String> {
        let (_, output_proxy_id) = self
            .subgraph_proxy_ids(node_id)
            .ok_or_else(|| format!("Node {:?} is not a SubGraphNode", node_id))?;
        let sg_path = NodePath::root().child(*node_id);
        let out_path = sg_path.child(output_proxy_id);

        let index = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let count = ns.input_slot_count(&out_path);
            if (0..count).any(|i| ns.input_slot(&out_path, i).map(|s| s.label) == Some(new_label))
                && old_label != new_label
            {
                return Err(format!(
                    "Output label {:?} already exists (uniqueness)",
                    new_label
                ));
            }
            (0..count)
                .find(|i| ns.input_slot(&out_path, *i).map(|s| s.label) == Some(old_label))
                .ok_or_else(|| format!("Output label {:?} not found", old_label))?
        };

        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.set_input_slot_label(&out_path, index, new_label);
        ns.set_output_slot_label(&out_path, index, new_label);
        // Rename external slot on parent.
        let parent_path = NodePath::root().child(*node_id);
        ns.set_output_slot_label(&parent_path, index, new_label);
        Ok(index)
    }

    /// Set the default value for a SubGraphNode input port AND mirror
    /// the same value onto the SubGraph's `InputProxy` input slot so the
    /// internals can read it at execute time.
    ///
    /// plan-010 §3.6: the SubGraphNode external slot is the wiring
    /// surface external callers see, but `InterfaceNode`'s pass-through
    /// tee reads from the proxy's own input slot. Without a mirror, the
    /// default never reaches the proxy and the internals downstream of
    /// `InputProxy.output[i]` execute with no value.
    ///
    /// **Atomicity**: both target slots are pre-validated to exist
    /// before any mutation; either both are updated or `Err` is
    /// returned with no partial state.
    ///
    /// **Dirty**: both the SubGraph path and the proxy path are marked
    /// changed so the executor's dirty planning includes the internals
    /// downstream of the proxy's output slots.
    pub fn set_subgraph_input_default(
        &self,
        node_id: &NodeId,
        input_index: usize,
        data: Data,
    ) -> Result<(), String> {
        let (input_proxy_id, _) = self.subgraph_proxy_ids(node_id).ok_or_else(|| {
            format!(
                "set_subgraph_input_default: SubGraph {:?} has no input proxy (schema corruption)",
                node_id
            )
        })?;
        let sg_path = NodePath::root().child(*node_id);
        let proxy_path = sg_path.child(input_proxy_id);

        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;

        // Pre-validate BOTH targets exist before any mutation.
        if ns.input_slot(&sg_path, input_index).is_none() {
            return Err(format!(
                "set_subgraph_input_default: external slot {} missing on SubGraph {:?}",
                input_index, node_id
            ));
        }
        if ns.input_slot(&proxy_path, input_index).is_none() {
            return Err(format!(
                "set_subgraph_input_default: InputProxy {:?} missing slot {} \
                 (schema mismatch with SubGraph external)",
                input_proxy_id, input_index
            ));
        }

        // Atomic mutate.
        let value = data.into_value();
        ns.input_slot_mut(&sg_path, input_index)
            .expect("validated above")
            .default_value = Some(Arc::clone(&value));
        ns.input_slot_mut(&proxy_path, input_index)
            .expect("validated above")
            .default_value = Some(value);

        // Dirty BOTH paths — see plan-010 §3.6.
        ns.mark_changed(&sg_path);
        ns.mark_changed(&proxy_path);
        Ok(())
    }

    /// Clear the default value of a SubGraphNode input port (set it back
    /// to `None`) AND clear the mirrored value on the `InputProxy` so
    /// internals stop seeing it. Companion to
    /// [`set_subgraph_input_default`].
    ///
    /// Same atomicity / dirty contract as `set_subgraph_input_default`.
    pub fn clear_subgraph_input_default(
        &self,
        node_id: &NodeId,
        input_index: usize,
    ) -> Result<(), String> {
        let (input_proxy_id, _) = self.subgraph_proxy_ids(node_id).ok_or_else(|| {
            format!(
                "clear_subgraph_input_default: SubGraph {:?} has no input proxy (schema corruption)",
                node_id
            )
        })?;
        let sg_path = NodePath::root().child(*node_id);
        let proxy_path = sg_path.child(input_proxy_id);

        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;

        if ns.input_slot(&sg_path, input_index).is_none() {
            return Err(format!(
                "clear_subgraph_input_default: external slot {} missing on SubGraph {:?}",
                input_index, node_id
            ));
        }
        if ns.input_slot(&proxy_path, input_index).is_none() {
            return Err(format!(
                "clear_subgraph_input_default: InputProxy {:?} missing slot {} \
                 (schema mismatch with SubGraph external)",
                input_proxy_id, input_index
            ));
        }

        ns.input_slot_mut(&sg_path, input_index)
            .expect("validated above")
            .default_value = None;
        ns.input_slot_mut(&proxy_path, input_index)
            .expect("validated above")
            .default_value = None;

        ns.mark_changed(&sg_path);
        ns.mark_changed(&proxy_path);
        Ok(())
    }
}

/// Append `label` to the `NodeData.locked` set of an `InterfaceNode` at `path`.
fn append_locked_label_at(graph: &NodeGraph, path: &NodePath, label: &str) -> Result<(), String> {
    let existing = {
        let ns = graph.node_states.read().map_err(|e| e.to_string())?;
        ns.get(path)
            .and_then(|s| s.data.as_ref().map(|d| d.share()))
    };
    let mut data = match existing {
        None => InterfaceNodeData::default(),
        Some(d) => match d.get_type() {
            DataType::Domain(name) if name == INTERFACE_NODE_DATA_DOMAIN => d
                .value::<InterfaceNodeData>()
                .map_err(|e| format!("InterfaceNodeData downcast: {e}"))?
                .clone(),
            other => {
                return Err(format!(
                    "InterfaceNode at {} has non-matching NodeData domain {:?}",
                    path, other
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
        .get_mut(path)
        .ok_or_else(|| format!("Node at {} disappeared mid-call", path))?;
    state.data = Some(Data::from_domain(data, INTERFACE_NODE_DATA_DOMAIN));
    ns.mark_changed(path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{DataType, NodeGraphWrite};

    #[test]
    fn test_is_subgraph_node() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let num = graph
            .create_node::<crate::NumberNode>()
            .expect("create num");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
            .expect("create subgraph");

        assert!(!graph.is_subgraph_node(&num));
        assert!(graph.is_subgraph_node(&sg));
    }

    #[test]
    fn test_subgraph_label() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "TestSG")
            .expect("create subgraph");

        assert_eq!(graph.subgraph_label(&sg), Some("TestSG".to_string()));
        assert!(graph.set_subgraph_label(&sg, "Custom"));
        assert_eq!(graph.subgraph_label(&sg), Some("Custom".to_string()));
    }

    #[test]
    fn test_subgraph_proxy_ids() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
            .expect("create subgraph");

        let (input_id, output_id) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        // Proxy IDs should be children of the SubGraphNode.
        let children = graph.subgraph_node_ids(&sg).expect("internal ids");
        assert!(children.contains(&input_id));
        assert!(children.contains(&output_id));
    }

    #[test]
    fn test_add_remove_subgraph_ports() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
            .expect("create subgraph");

        // Add input
        graph
            .add_subgraph_input(&sg, "In1", DataType::Number)
            .expect("add input");

        // Add output
        graph
            .add_subgraph_output(&sg, "Out1", DataType::Number)
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

    /// Verify that renaming a SubGraphNode input by label keeps the
    /// slot index unchanged.
    #[test]
    fn test_rename_subgraph_input_preserves_edges() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
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

    /// Verify that renaming a SubGraphNode output renames both proxy
    /// slots.
    #[test]
    fn test_rename_subgraph_output_renames_proxy() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
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
    }

    // -----------------------------------------------------------------
    // plan-010 §3.6 — SubGraph external default → InputProxy mirror
    // -----------------------------------------------------------------

    #[test]
    fn set_subgraph_input_default_mirrors_to_input_proxy() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
            .expect("create subgraph");
        graph
            .add_subgraph_input(&sg, "x", DataType::Number)
            .expect("add input");

        let value = crate::Data::new(7.0_f64).expect("Data::new");
        graph
            .set_subgraph_input_default(&sg, 0, value.clone())
            .expect("set default");

        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        let sg_path = crate::NodePath::root().child(sg);
        let proxy_path = sg_path.child(input_proxy_id);

        let ns = graph.node_states().read().expect("read");
        let ext = ns.input_slot(&sg_path, 0).expect("ext slot");
        let prx = ns.input_slot(&proxy_path, 0).expect("proxy slot");
        assert!(ext.default_value.is_some(), "external default written");
        assert!(prx.default_value.is_some(), "proxy default mirrored");
    }

    #[test]
    fn clear_subgraph_input_default_clears_input_proxy_too() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
            .expect("create subgraph");
        graph
            .add_subgraph_input(&sg, "x", DataType::Number)
            .expect("add input");
        graph
            .set_subgraph_input_default(&sg, 0, crate::Data::new(7.0_f64).unwrap())
            .expect("set default");

        graph
            .clear_subgraph_input_default(&sg, 0)
            .expect("clear default");

        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        let sg_path = crate::NodePath::root().child(sg);
        let proxy_path = sg_path.child(input_proxy_id);
        let ns = graph.node_states().read().expect("read");
        assert!(ns.input_slot(&sg_path, 0).unwrap().default_value.is_none());
        assert!(ns.input_slot(&proxy_path, 0).unwrap().default_value.is_none());
    }

    #[test]
    fn set_subgraph_input_default_leaves_no_partial_state_on_proxy_slot_missing() {
        // Corrupt the proxy slot count by removing it directly so the mirror
        // step fails. The external slot must NOT be mutated when the mirror
        // can't land.
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
            .expect("create subgraph");
        graph
            .add_subgraph_input(&sg, "x", DataType::Number)
            .expect("add input");

        // Snapshot the (None) external default before the corruption.
        let sg_path = crate::NodePath::root().child(sg);
        {
            let ns = graph.node_states().read().expect("read");
            assert!(ns.input_slot(&sg_path, 0).unwrap().default_value.is_none());
        }

        // Remove the proxy input slot to simulate schema corruption.
        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        let proxy_path = sg_path.child(input_proxy_id);
        {
            let mut ns = graph.node_states().write().expect("write");
            ns.remove_input_slot(&proxy_path, 0);
        }

        let err = graph
            .set_subgraph_input_default(&sg, 0, crate::Data::new(7.0_f64).unwrap())
            .expect_err("missing proxy slot must Err");
        assert!(err.contains("InputProxy"), "error mentions InputProxy: {err}");

        // External slot must STILL be untouched.
        let ns = graph.node_states().read().expect("read");
        assert!(
            ns.input_slot(&sg_path, 0).unwrap().default_value.is_none(),
            "external slot must not be mutated when mirror fails",
        );
    }

    /// plan-010 §3.6: after a default change, both the SubGraph path AND
    /// the InputProxy path are marked changed so the executor's dirty
    /// plan re-runs the proxy (and the internals downstream of its
    /// output slots). Without proxy-side dirty marking the internal mesh
    /// would stay stale.
    #[test]
    fn set_subgraph_input_default_marks_both_paths_changed() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .add_subgraph_at(&crate::NodePath::root(), "Test")
            .expect("create subgraph");
        graph
            .add_subgraph_input(&sg, "x", DataType::Number)
            .expect("add input");

        // Drain any pre-existing dirty set so we only observe THIS call.
        {
            let mut ns = graph.node_states().write().expect("write");
            let _ = ns.drain_changed_nodes();
        }

        graph
            .set_subgraph_input_default(&sg, 0, crate::Data::new(11.0_f64).unwrap())
            .expect("set default");

        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        let sg_path = crate::NodePath::root().child(sg);
        let proxy_path = sg_path.child(input_proxy_id);

        let dirty = {
            let mut ns = graph.node_states().write().expect("write");
            ns.drain_changed_nodes()
        };
        assert!(dirty.contains(&sg_path), "sg_path must be dirty");
        assert!(
            dirty.contains(&proxy_path),
            "proxy_path must be dirty (internals re-execute trigger)"
        );
    }
}
