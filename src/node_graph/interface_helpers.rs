//! Convenience APIs for [`InterfaceNode`] port management.
//!
//! plan-005 Task 2. These helpers manipulate a single `InterfaceNode`'s
//! dynamic port set (label + type + locked-flag), preserving the
//! invariant that **each port occupies exactly one input slot AND one
//! output slot at the same index**. They do NOT touch the parent
//! `SubGraphNode`'s external schema — that's the job of the
//! `add_subgraph_input` family (Task 5).
//!
//! Resolved (t): every API pre-validates the target node's NodeData
//! domain (must be `InterfaceNodeData` if present) BEFORE mutating any
//! slot. Absent NodeData is treated as the default (empty locked set);
//! a non-matching domain is `Err` and leaves slots untouched.

use crate::{
    Data, DataType, InterfaceNode, InterfaceNodeData, NodeGraph, NodeId, NodePath,
    INTERFACE_NODE_DATA_DOMAIN,
};

impl NodeGraph {
    /// Add a port to an `InterfaceNode`.
    ///
    /// Adds one input slot and one mirrored output slot at the same
    /// index, both labelled `label` and typed `data_type`. When
    /// `locked == true` the label is appended to the node's
    /// `NodeData.locked` set so subsequent remove / rename are
    /// rejected. Returns the (shared) port index.
    pub fn add_interface_port(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
        locked: bool,
    ) -> Result<usize, String> {
        // 1. Verify target node exists AND is an InterfaceNode.
        let entity = self
            .node_manager
            .get_at(&NodePath::root().child(*node_id))
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        {
            let guard = entity.read().map_err(|e| e.to_string())?;
            if guard.as_any().downcast_ref::<InterfaceNode>().is_none() {
                return Err(format!(
                    "Node {:?} is not an InterfaceNode (got {})",
                    node_id,
                    guard.node_name()
                ));
            }
        }

        // 2. Pre-validate the existing NodeData domain. If a wrong
        //    domain is stamped on the node, reject BEFORE any slot
        //    mutation so we never leave half-state behind (resolved (t)).
        let existing_data = self.node_data(node_id);
        let existing_interface_data = match &existing_data {
            None => None,
            Some(d) => match d.get_type() {
                DataType::Domain(name) if name == INTERFACE_NODE_DATA_DOMAIN => {
                    d.value::<InterfaceNodeData>().ok().cloned()
                }
                other => {
                    return Err(format!(
                        "InterfaceNode {:?} has non-matching NodeData domain {:?}; expected Domain(\"{}\")",
                        node_id, other, INTERFACE_NODE_DATA_DOMAIN
                    ));
                }
            },
        };

        // 3. Add input + output slots at the same index.
        let index = {
            let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
            let node_path = NodePath::root().child(*node_id);
            let in_idx = ns.add_input_slot(&node_path, label, data_type, Some(1));
            let out_idx = ns.add_output_slot(&node_path, label, data_type);
            // Invariant: input and output slot indices stay in lock-step.
            assert_eq!(
                in_idx, out_idx,
                "InterfaceNode {:?}: input slot index {} != output slot index {}",
                node_id, in_idx, out_idx
            );
            in_idx
        };

        // 4. If locked, write back NodeData with the label appended to
        //    the locked set. Insert a fresh default when no NodeData
        //    was present.
        if locked {
            let mut data = existing_interface_data.unwrap_or_default();
            data.locked.insert(label.to_string());
            let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
            let state = guard
                .get_mut(&NodePath::root().child(*node_id))
                .ok_or_else(|| format!("Node {:?} disappeared mid-call", node_id))?;
            state.data = Some(Data::from_domain(data, INTERFACE_NODE_DATA_DOMAIN));
            guard.mark_changed(&NodePath::root().child(*node_id));
        }

        Ok(index)
    }

    /// Remove a port from an `InterfaceNode` by label.
    ///
    /// Removes both the input and output slots at the matching index.
    /// Rejects when the label is in the node's `NodeData.locked` set.
    pub fn remove_interface_port(&self, node_id: &NodeId, label: &str) -> Result<(), String> {
        // 1. Verify target node exists AND is an InterfaceNode.
        let entity = self
            .node_manager
            .get_at(&NodePath::root().child(*node_id))
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        {
            let guard = entity.read().map_err(|e| e.to_string())?;
            if guard.as_any().downcast_ref::<InterfaceNode>().is_none() {
                return Err(format!(
                    "Node {:?} is not an InterfaceNode (got {})",
                    node_id,
                    guard.node_name()
                ));
            }
        }

        // 2. Pre-validate NodeData domain + locked check.
        let existing_data = self.node_data(node_id);
        let existing_interface_data = match &existing_data {
            None => None,
            Some(d) => match d.get_type() {
                DataType::Domain(name) if name == INTERFACE_NODE_DATA_DOMAIN => {
                    d.value::<InterfaceNodeData>().ok().cloned()
                }
                other => {
                    return Err(format!(
                        "InterfaceNode {:?} has non-matching NodeData domain {:?}",
                        node_id, other
                    ));
                }
            },
        };
        if let Some(d) = &existing_interface_data {
            if d.locked.contains(label) {
                return Err(format!(
                    "InterfaceNode port '{}' is locked and cannot be removed",
                    label
                ));
            }
        }

        // 3. Find the slot index by label (input side; output side
        //    mirrors at the same index).
        let node_path = NodePath::root().child(*node_id);
        let index = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            (0..ns.input_slot_count(&node_path))
                .find(|i| ns.input_slot(&node_path, *i).map(|s| s.label) == Some(label))
                .ok_or_else(|| {
                    format!(
                        "InterfaceNode {:?} has no port labelled '{}'",
                        node_id, label
                    )
                })?
        };

        // 4. Drop edges connected to either side, then remove both
        //    slots. NodeState's existing remove helpers shift the
        //    higher-indexed slot ids down so the input/output
        //    invariant survives.
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.remove_input_slot(&node_path, index);
        ns.remove_output_slot(&node_path, index);
        ns.mark_changed(&NodePath::root().child(*node_id));

        Ok(())
    }

    /// Rename a port on an `InterfaceNode`. Slot ids are preserved on
    /// both input and output sides so existing edges survive.
    pub fn rename_interface_port(
        &self,
        node_id: &NodeId,
        old: &str,
        new: &'static str,
    ) -> Result<(), String> {
        // 1. Verify target node.
        let entity = self
            .node_manager
            .get_at(&NodePath::root().child(*node_id))
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        {
            let guard = entity.read().map_err(|e| e.to_string())?;
            if guard.as_any().downcast_ref::<InterfaceNode>().is_none() {
                return Err(format!(
                    "Node {:?} is not an InterfaceNode (got {})",
                    node_id,
                    guard.node_name()
                ));
            }
        }

        // 2. Pre-validate NodeData + locked check on the old label.
        let existing_data = self.node_data(node_id);
        let existing_interface_data = match &existing_data {
            None => None,
            Some(d) => match d.get_type() {
                DataType::Domain(name) if name == INTERFACE_NODE_DATA_DOMAIN => {
                    d.value::<InterfaceNodeData>().ok().cloned()
                }
                other => {
                    return Err(format!(
                        "InterfaceNode {:?} has non-matching NodeData domain {:?}",
                        node_id, other
                    ));
                }
            },
        };
        if let Some(d) = &existing_interface_data {
            if d.locked.contains(old) {
                return Err(format!(
                    "InterfaceNode port '{}' is locked and cannot be renamed",
                    old
                ));
            }
        }

        // 3. Locate the slot index by old label + check new label is unused.
        let node_path = NodePath::root().child(*node_id);
        let index = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            // Reject if the new label already exists.
            let has_new = (0..ns.input_slot_count(&node_path))
                .any(|i| ns.input_slot(&node_path, i).map(|s| s.label) == Some(new));
            if has_new {
                return Err(format!(
                    "InterfaceNode {:?} already has a port labelled '{}'",
                    node_id, new
                ));
            }
            (0..ns.input_slot_count(&node_path))
                .find(|i| ns.input_slot(&node_path, *i).map(|s| s.label) == Some(old))
                .ok_or_else(|| {
                    format!("InterfaceNode {:?} has no port labelled '{}'", node_id, old)
                })?
        };

        // 4. Set labels on both sides at the matching index. Slot ids
        //    are preserved so edges survive.
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.set_input_slot_label(&node_path, index, new);
        ns.set_output_slot_label(&node_path, index, new);
        ns.mark_changed(&NodePath::root().child(*node_id));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Data, NodeGraph, NodeGraphWrite};
    use std::collections::HashSet;

    fn fresh_graph_with_interface_node() -> (NodeGraph, NodeId) {
        let mut g = NodeGraph::new().expect("NodeGraph::new");
        let id = g
            .create_node_by_name("InterfaceNode")
            .expect("create InterfaceNode");
        (g, id)
    }

    fn stamp_interface_data(
        g: &mut NodeGraph,
        id: &NodeId,
        direction: crate::InterfaceDirection,
        locked: &[&str],
    ) {
        let data = InterfaceNodeData {
            direction,
            locked: locked.iter().map(|s| (*s).to_string()).collect(),
        };
        g.update_node_data(id, Data::from_domain(data, INTERFACE_NODE_DATA_DOMAIN))
            .expect("stamp InterfaceNodeData");
    }

    #[test]
    fn add_unlocked_creates_matching_input_and_output_slot_pair() {
        let (g, id) = fresh_graph_with_interface_node();
        let idx = g
            .add_interface_port(&id, "foo", DataType::Number, false)
            .expect("add_interface_port");
        assert_eq!(idx, 0);
        assert_eq!(g.input_slot_count(&id), 1);
        assert_eq!(g.output_slot_count(&id), 1);
        assert_eq!(g.input_slot_label(&id, 0), Some("foo"));
        assert_eq!(g.output_slot_label(&id, 0), Some("foo"));
        assert_eq!(g.input_slot_data_type(&id, 0), Some(DataType::Number));
        assert_eq!(g.output_slot_data_type(&id, 0), Some(DataType::Number));
    }

    #[test]
    fn add_on_non_interface_node_returns_err() {
        let mut g = NodeGraph::new().expect("NodeGraph::new");
        let sg = g.create_node_by_name("SubGraph").expect("create SubGraph");
        let err = g
            .add_interface_port(&sg, "x", DataType::Number, false)
            .expect_err("must reject non-InterfaceNode");
        assert!(err.contains("not an InterfaceNode"), "got: {err}");
    }

    #[test]
    fn add_locked_inserts_fresh_node_data_when_absent() {
        let (g, id) = fresh_graph_with_interface_node();
        g.add_interface_port(&id, "locked_one", DataType::Number, true)
            .expect("add locked");
        let d = g
            .node_data(&id)
            .expect("NodeData must exist after locked add");
        let parsed: &InterfaceNodeData = d.value().expect("InterfaceNodeData payload");
        assert!(parsed.locked.contains("locked_one"));
    }

    #[test]
    fn add_locked_appends_to_existing_locked_set() {
        let (mut g, id) = fresh_graph_with_interface_node();
        stamp_interface_data(
            &mut g,
            &id,
            crate::InterfaceDirection::Input,
            &["pre_existing"],
        );
        g.add_interface_port(&id, "new_locked", DataType::Number, true)
            .expect("add locked");
        let d = g.node_data(&id).expect("NodeData");
        let parsed: &InterfaceNodeData = d.value().expect("InterfaceNodeData");
        let expected: HashSet<String> = ["pre_existing", "new_locked"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(parsed.locked, expected);
    }

    #[test]
    fn add_on_wrong_domain_node_data_returns_err_without_mutation() {
        let (mut g, id) = fresh_graph_with_interface_node();
        g.update_node_data(&id, Data::from_domain(42_u32, "SomeOtherDomain"))
            .expect("stamp wrong-domain data");
        let err = g
            .add_interface_port(&id, "x", DataType::Number, true)
            .expect_err("must reject wrong-domain NodeData");
        assert!(err.contains("non-matching NodeData domain"), "got: {err}");
        // Slots were not touched.
        assert_eq!(g.input_slot_count(&id), 0);
        assert_eq!(g.output_slot_count(&id), 0);
    }

    #[test]
    fn remove_unlocked_drops_both_slot_sides() {
        let (g, id) = fresh_graph_with_interface_node();
        g.add_interface_port(&id, "rmme", DataType::Number, false)
            .expect("add");
        g.remove_interface_port(&id, "rmme").expect("remove");
        assert_eq!(g.input_slot_count(&id), 0);
        assert_eq!(g.output_slot_count(&id), 0);
    }

    #[test]
    fn remove_locked_returns_err_without_mutation() {
        let (g, id) = fresh_graph_with_interface_node();
        g.add_interface_port(&id, "locked", DataType::Number, true)
            .expect("add locked");
        let err = g
            .remove_interface_port(&id, "locked")
            .expect_err("locked rejection");
        assert!(err.contains("locked and cannot be removed"), "got: {err}");
        assert_eq!(g.input_slot_count(&id), 1);
        assert_eq!(g.output_slot_count(&id), 1);
    }

    #[test]
    fn remove_unknown_label_returns_err() {
        let (g, id) = fresh_graph_with_interface_node();
        let err = g
            .remove_interface_port(&id, "nope")
            .expect_err("must err on missing label");
        assert!(err.contains("has no port labelled"), "got: {err}");
    }

    #[test]
    fn rename_renames_both_sides_keeping_slot_ids() {
        let (g, id) = fresh_graph_with_interface_node();
        g.add_interface_port(&id, "old", DataType::Number, false)
            .expect("add");
        let in_id_before = g.input_slot_id(&id, 0).expect("slot id");
        g.rename_interface_port(&id, "old", "new").expect("rename");
        assert_eq!(g.input_slot_label(&id, 0), Some("new"));
        assert_eq!(g.output_slot_label(&id, 0), Some("new"));
        let in_id_after = g.input_slot_id(&id, 0).expect("slot id");
        assert_eq!(
            in_id_before, in_id_after,
            "slot id must be stable across rename"
        );
    }

    #[test]
    fn rename_locked_returns_err() {
        let (g, id) = fresh_graph_with_interface_node();
        g.add_interface_port(&id, "locked", DataType::Number, true)
            .expect("add locked");
        let err = g
            .rename_interface_port(&id, "locked", "other")
            .expect_err("locked rename rejection");
        assert!(err.contains("locked and cannot be renamed"), "got: {err}");
    }

    #[test]
    fn rename_to_existing_label_returns_err() {
        let (g, id) = fresh_graph_with_interface_node();
        g.add_interface_port(&id, "a", DataType::Number, false)
            .expect("add a");
        g.add_interface_port(&id, "b", DataType::Number, false)
            .expect("add b");
        let err = g
            .rename_interface_port(&id, "a", "b")
            .expect_err("must reject duplicate target");
        assert!(err.contains("already has a port labelled"), "got: {err}");
    }

    #[test]
    fn remove_on_absent_node_data_treats_locked_as_empty_and_succeeds() {
        let (g, id) = fresh_graph_with_interface_node();
        g.add_interface_port(&id, "x", DataType::Number, false)
            .expect("add unlocked");
        // No NodeData was inserted (locked=false path); remove must
        // still succeed without inventing a locked entry.
        assert!(g.node_data(&id).is_none());
        g.remove_interface_port(&id, "x").expect("remove");
    }
}
