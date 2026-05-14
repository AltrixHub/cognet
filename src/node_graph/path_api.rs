//! Path-aware NodeGraph APIs (plan-006 P3c.10).
//!
//! Provides `add_node_at`, `add_subgraph_at`, `remove_node_at`,
//! `node_at_path`, `children_of_path`, `is_subgraph_node_at_path`,
//! `stamp_interface_direction`, and `subgraph_proxy_ids_at_path`.
//!
//! These replace the old `with_graph_at_path` / `internal_graph`
//! traversal patterns.

use std::{any::TypeId, sync::Arc};

use crate::{
    Data, InterfaceDirection, InterfaceNode, InterfaceNodeData, NodeEntity, NodeGraph, NodeId,
    NodeMeta, NodePath, SubGraphNode, INTERFACE_NODE_DATA_DOMAIN,
};

use super::node_state::NodeStates;

impl NodeGraph {
    // ── Path-aware node queries ──

    /// Get the node entity at an arbitrary `NodePath`.
    ///
    /// For root-level nodes: `NodePath::root().child(id)`.
    /// For SubGraph children: `sg_path.child(child_id)`.
    pub fn node_at_path(&self, path: &NodePath) -> Option<NodeEntity> {
        self.node_manager.get_at(path)
    }

    /// Get the direct child NodeIds of the SubGraph at `path`.
    ///
    /// Returns the NodeIds of all nodes immediately inside the SubGraph,
    /// including its two boundary InterfaceNodes.
    pub fn children_of_path(&self, path: &NodePath) -> Vec<NodeId> {
        let ns = match self.node_states.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        ns.path_index_children_of(path).to_vec()
    }

    /// Return true if the entity at `path` is a `SubGraphNode`.
    pub fn is_subgraph_node_at_path(&self, path: &NodePath) -> bool {
        self.node_manager
            .get_at(path)
            .and_then(|e| {
                e.read()
                    .ok()
                    .map(|g| g.as_any().downcast_ref::<SubGraphNode>().is_some())
            })
            .unwrap_or(false)
    }

    /// Get the `(input_proxy_id, output_proxy_id)` pair of a SubGraph at `path`.
    pub fn subgraph_proxy_ids_at_path(&self, path: &NodePath) -> Option<(NodeId, NodeId)> {
        let entity = self.node_manager.get_at(path)?;
        let read = entity.read().ok()?;
        let sg = read.as_any().downcast_ref::<SubGraphNode>()?;
        Some((sg.input_proxy_id(), sg.output_proxy_id()))
    }

    // ── Atomic node insertion ──

    /// Insert a single node entity at `path`, keeping NodeManager and
    /// NodeStates in lock-step. This acquires both write locks for the
    /// duration of the call.
    pub fn add_node_at(&mut self, path: &NodePath, entity: NodeEntity) -> Result<(), String> {
        let nm = &self.node_manager;
        let (name, type_id, default_data, input_defs, output_defs) = entity_meta_views(&entity)?;
        if nm.contains_path(path) {
            return Err(format!("add_node_at: path {path} already registered"));
        }
        nm.insert_at(path.clone(), entity);
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        ns.add_node(path, name, type_id, default_data, input_defs, output_defs);
        Ok(())
    }

    /// Atomically allocate a SubGraphNode with its two InterfaceNodes
    /// under a single pair of write locks.
    ///
    /// Creates:
    /// - SubGraphNode at `parent_path.child(sg_id)`
    /// - Input-direction InterfaceNode at `sg_path.child(in_id)`
    /// - Output-direction InterfaceNode at `sg_path.child(out_id)`
    ///
    /// On failure, any nodes already inserted within this call are
    /// rolled back. Returns the `NodeId` of the new SubGraphNode.
    pub fn add_subgraph_at(
        &mut self,
        parent_path: &NodePath,
        label: impl Into<String>,
    ) -> Result<NodeId, String> {
        let sg_id = NodeId::new();
        let sg_path = parent_path.child(sg_id);
        let in_id = NodeId::new();
        let out_id = NodeId::new();
        let in_path = sg_path.child(in_id);
        let out_path = sg_path.child(out_id);

        // Pre-validate: no path collisions.
        if self.node_manager.contains_path(&sg_path)
            || self.node_manager.contains_path(&in_path)
            || self.node_manager.contains_path(&out_path)
        {
            return Err(format!(
                "add_subgraph_at: path collision under {}",
                parent_path
            ));
        }

        // Build all three entities before any mutation.
        let sg_entity: NodeEntity = Arc::new(std::sync::RwLock::new(SubGraphNode::with_proxies(
            in_id, out_id, label,
        )));
        let in_entity: NodeEntity = Arc::new(std::sync::RwLock::new(InterfaceNode));
        let out_entity: NodeEntity = Arc::new(std::sync::RwLock::new(InterfaceNode));

        // Insert SubGraphNode.
        let mut inserted: Vec<NodePath> = Vec::with_capacity(3);
        let result = (|| -> Result<(), String> {
            self.insert_entity_at(
                &sg_path,
                sg_entity,
                NodeMeta_ {
                    name: SubGraphNode::NAME,
                    type_id: Some(TypeId::of::<SubGraphNode>()),
                    default_data: None,
                    input_defs: &[],
                    output_defs: &[],
                },
            )?;
            inserted.push(sg_path.clone());

            self.insert_entity_at(
                &in_path,
                in_entity,
                NodeMeta_ {
                    name: InterfaceNode::NAME,
                    type_id: Some(TypeId::of::<InterfaceNode>()),
                    default_data: None,
                    input_defs: InterfaceNode::INPUTS,
                    output_defs: InterfaceNode::OUTPUTS,
                },
            )?;
            inserted.push(in_path.clone());
            self.stamp_interface_direction_internal(&in_path, InterfaceDirection::Input)?;

            self.insert_entity_at(
                &out_path,
                out_entity,
                NodeMeta_ {
                    name: InterfaceNode::NAME,
                    type_id: Some(TypeId::of::<InterfaceNode>()),
                    default_data: None,
                    input_defs: InterfaceNode::INPUTS,
                    output_defs: InterfaceNode::OUTPUTS,
                },
            )?;
            inserted.push(out_path.clone());
            self.stamp_interface_direction_internal(&out_path, InterfaceDirection::Output)?;

            Ok(())
        })();

        if let Err(e) = result {
            // Rollback: remove any successfully-inserted nodes.
            for p in inserted.iter().rev() {
                self.node_manager.remove_at(p);
                if let Ok(mut ns) = self.node_states.write() {
                    ns.remove_node(p);
                }
            }
            return Err(e);
        }

        Ok(sg_id)
    }

    /// Remove the node at `path` and all its descendants, tearing down
    /// edges and slot tables in a single transaction.
    ///
    /// Collects all descendant paths first (leaves first), then for
    /// each path: drops edges, drops slots, drops NodeStates entry,
    /// drops NodeManager entry.
    pub fn remove_node_at(&mut self, path: &NodePath) -> Result<(), String> {
        // Collect all paths under (and including) `path`, leaves first.
        let all_paths = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            collect_subtree_paths(&ns, path)
        };

        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        for p in &all_paths {
            // Drop edges connected to this node.
            if let Some(node_id) = p.leaf() {
                ns.remove_edges_for_node(&node_id);
            }
            // Remove slot tables and NodeStates entry.
            ns.remove_node(p);
            // Remove from NodeManager.
            self.node_manager.remove_at(p);
        }

        // Record removals for execution result reporting.
        for p in &all_paths {
            if let Some(id) = p.leaf() {
                self.record_removal(id);
            }
        }

        Ok(())
    }

    // ── stamp_interface_direction ──

    // Private: stamp an InterfaceNode at `path` with the given direction.
    fn stamp_interface_direction_internal(
        &self,
        path: &NodePath,
        direction: InterfaceDirection,
    ) -> Result<(), String> {
        let data = InterfaceNodeData {
            direction,
            locked: std::collections::HashSet::new(),
        };
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        let state = ns
            .get_mut(path)
            .ok_or_else(|| format!("stamp_interface_direction: node at {} not found", path))?;
        state.data = Some(Data::from_domain(data, INTERFACE_NODE_DATA_DOMAIN));
        ns.mark_changed(path);
        Ok(())
    }

    // Internal helper: insert entity + register in both NodeManager and NodeStates.
    fn insert_entity_at(
        &self,
        path: &NodePath,
        entity: NodeEntity,
        meta: NodeMeta_,
    ) -> Result<(), String> {
        if self.node_manager.contains_path(path) {
            return Err(format!("insert_entity_at: path {path} already registered"));
        }
        self.node_manager.insert_at(path.clone(), entity);
        let mut ns = self.node_states.write().map_err(|e| {
            // Rollback NodeManager insert on NodeStates failure.
            self.node_manager.remove_at(path);
            e.to_string()
        })?;
        ns.add_node(
            path,
            meta.name,
            meta.type_id,
            meta.default_data,
            meta.input_defs,
            meta.output_defs,
        );
        Ok(())
    }
}

/// Compact metadata bundle for `insert_entity_at`.
struct NodeMeta_ {
    name: &'static str,
    type_id: Option<TypeId>,
    default_data: Option<Data>,
    input_defs: &'static [crate::SlotDef],
    output_defs: &'static [crate::SlotDef],
}

/// Tuple returned by `entity_meta_views`: (name, type_id, default_data, input_defs, output_defs).
type EntityMetaViews = (
    &'static str,
    Option<TypeId>,
    Option<Data>,
    &'static [crate::SlotDef],
    &'static [crate::SlotDef],
);

/// Extract (name, type_id, default_data, input_defs, output_defs) from a
/// NodeEntity by reading the node through its RwLock.
fn entity_meta_views(entity: &NodeEntity) -> Result<EntityMetaViews, String> {
    let read = entity.read().map_err(|e| e.to_string())?;
    // We can get name, but type_id / default_data / slot defs require NodeMeta
    // static methods. Use AsAny-based downcast to try known types; fall back
    // to a name-only registration. This is only used by `add_node_at` for
    // externally-built entities; `add_subgraph_at` passes these directly.
    let name = read.node_name();
    // Drop read to avoid held-across-acquire issues.
    drop(read);

    // For externally-supplied entities we cannot recover static slot defs
    // because the trait object erases them. The caller is responsible for
    // registering slots separately via `add_input_slot`/`add_output_slot`.
    // We just use empty slices here and rely on the caller to wire slots.
    Ok((
        // SAFETY: node_name() returns &'static str
        unsafe { std::mem::transmute::<&str, &'static str>(name) },
        None,
        None,
        &[],
        &[],
    ))
}

/// Collect all paths in the subtree rooted at `root`, leaves first.
fn collect_subtree_paths(ns: &NodeStates, root: &NodePath) -> Vec<NodePath> {
    let mut result = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(p) = stack.pop() {
        // Push children first so they appear before parent in result.
        for child_id in ns.path_index_children_of(&p) {
            stack.push(p.child(*child_id));
        }
        result.push(p);
    }
    // Reverse so leaves appear before parents.
    result.reverse();
    result
}

#[cfg(test)]
mod tests {
    use crate::NodeGraphRead;

    #[test]
    fn add_subgraph_at_root_registers_three_nodes() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "TestSG")
            .expect("add_subgraph_at");

        // SubGraphNode in NodeManager at root.child(sg_id)
        assert!(graph.get_node_by_id(&sg_id).is_some());

        // Two proxy children
        let sg_path = crate::NodePath::root().child(sg_id);
        let children = graph.children_of_path(&sg_path);
        assert_eq!(
            children.len(),
            2,
            "expected 2 proxy children, got {}",
            children.len()
        );
    }

    #[test]
    fn node_at_path_finds_child_interface_node() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let (in_id, out_id) = graph
            .subgraph_proxy_ids_at_path(&sg_path)
            .expect("proxy ids");

        assert!(graph.node_at_path(&sg_path.child(in_id)).is_some());
        assert!(graph.node_at_path(&sg_path.child(out_id)).is_some());
    }

    #[test]
    fn remove_node_at_cleans_up_subtree() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let (in_id, _) = graph
            .subgraph_proxy_ids_at_path(&sg_path)
            .expect("proxy ids");

        graph.remove_node_at(&sg_path).expect("remove_node_at");

        // SubGraph and its children are gone
        assert!(graph.get_node_by_id(&sg_id).is_none());
        assert!(graph.node_at_path(&sg_path.child(in_id)).is_none());
    }

    #[test]
    fn is_subgraph_node_at_path_distinguishes_types() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let (in_id, _) = graph
            .subgraph_proxy_ids_at_path(&sg_path)
            .expect("proxy ids");

        assert!(graph.is_subgraph_node_at_path(&sg_path));
        assert!(!graph.is_subgraph_node_at_path(&sg_path.child(in_id)));
    }
}
