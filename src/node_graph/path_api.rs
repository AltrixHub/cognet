//! Path-aware NodeGraph APIs.
//!
//! Provides `add_node_at`, `add_subgraph_at`, `remove_node_at`,
//! `node_at_path`, `children_of_path`, `is_subgraph_node_at_path`,
//! `stamp_interface_direction`, `subgraph_proxy_ids_at_path`,
//! `subgraph_external_input_slot`, and `subgraph_external_output_slot`.
//!
//! These replace the old `with_graph_at_path` / `internal_graph`
//! traversal patterns.

use std::{any::TypeId, sync::Arc};

use crate::{
    Data, DataType, InputSlotId, InterfaceDirection, InterfaceNode, InterfaceNodeData, NodeEntity,
    NodeGraph, NodeId, NodeMeta, NodePath, OutputSlotId, SlotDef, SubGraphNode,
    INTERFACE_NODE_DATA_DOMAIN,
};

use super::node_graph_api::{extract_fields_from_data, extract_fields_from_default_value};
use super::node_state::NodeStates;

impl NodeGraph {
    // ── Locked helpers (take already-acquired NodeStates guard) ──

    /// Insert a node entity into NodeManager and into an already-locked
    /// `NodeStates`.  The caller is responsible for pre-validating that
    /// `path` is not already occupied.
    fn add_node_at_locked(
        &self,
        ns: &mut NodeStates,
        path: &NodePath,
        entity: NodeEntity,
        meta: NodeMeta_,
    ) -> Result<(), String> {
        if self.node_manager.contains_path(path) {
            return Err(format!(
                "add_node_at_locked: path {path} already registered"
            ));
        }
        self.node_manager.insert_at(path.clone(), entity);
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

    /// Stamp an InterfaceNode direction inside an already-locked `NodeStates`.
    fn stamp_interface_direction_locked(
        ns: &mut NodeStates,
        path: &NodePath,
        direction: InterfaceDirection,
    ) -> Result<(), String> {
        let data = InterfaceNodeData {
            direction,
            locked: std::collections::HashSet::new(),
        };
        let state = ns
            .get_mut(path)
            .ok_or_else(|| format!("stamp_interface_direction: node at {} not found", path))?;
        state.data = Some(Data::from_domain(data, INTERFACE_NODE_DATA_DOMAIN));
        ns.mark_changed(path);
        Ok(())
    }

    /// Remove a node from NodeManager and from an already-locked `NodeStates`.
    fn remove_node_at_locked(&self, ns: &mut NodeStates, path: &NodePath) -> Result<(), String> {
        ns.remove_node(path);
        self.node_manager.remove_at(path);
        Ok(())
    }

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

    // ── SubGraph external slot helpers (plan-006 C17 Step 11.5c) ──

    /// Return the `(NodePath, InputSlotId)` pair that an external edge targets
    /// when wiring into input port `port_index` of the SubGraph at `sg_path`.
    ///
    /// The external input surface of a SubGraphNode is the input-direction
    /// `InterfaceNode` at `sg_path.child(input_proxy_id)`. Each slot on that
    /// InterfaceNode is one external input port. This helper resolves the
    /// concrete `(path, slot_id)` pair so callers can construct `Edge` values
    /// without having to navigate the proxy manually.
    ///
    /// Returns `None` if `sg_path` is not a SubGraphNode or the slot index is
    /// out of range.
    pub fn subgraph_external_input_slot(
        &self,
        sg_path: &NodePath,
        port_index: usize,
    ) -> Option<(NodePath, InputSlotId)> {
        let entity = self.node_manager.get_at(sg_path)?;
        let read = entity.read().ok()?;
        let sg = read.as_any().downcast_ref::<SubGraphNode>()?;
        let in_path = sg_path.child(sg.input_proxy_id());
        drop(read);
        let states = self.node_states.read().ok()?;
        states
            .input_slot(&in_path, port_index)
            .map(|s| (in_path.clone(), s.id))
    }

    /// Return the `(NodePath, OutputSlotId)` pair that an external edge targets
    /// when wiring out of output port `port_index` of the SubGraph at `sg_path`.
    ///
    /// The external output surface of a SubGraphNode is the output-direction
    /// `InterfaceNode` at `sg_path.child(output_proxy_id)`. The *output* slots
    /// of that InterfaceNode are the external outputs (the node tees its inputs
    /// to those output slots). This helper resolves the concrete
    /// `(path, slot_id)` pair.
    ///
    /// Returns `None` if `sg_path` is not a SubGraphNode or the slot index is
    /// out of range.
    pub fn subgraph_external_output_slot(
        &self,
        sg_path: &NodePath,
        port_index: usize,
    ) -> Option<(NodePath, OutputSlotId)> {
        let entity = self.node_manager.get_at(sg_path)?;
        let read = entity.read().ok()?;
        let sg = read.as_any().downcast_ref::<SubGraphNode>()?;
        let out_path = sg_path.child(sg.output_proxy_id());
        drop(read);
        let states = self.node_states.read().ok()?;
        states
            .output_slot(&out_path, port_index)
            .map(|s| (out_path.clone(), s.id))
    }

    // ── Atomic node insertion ──

    /// Insert a single node entity at `path`, keeping NodeManager and
    /// NodeStates in lock-step.
    pub fn add_node_at(&mut self, path: &NodePath, entity: NodeEntity) -> Result<(), String> {
        let (name, type_id, default_data, input_defs, output_defs) = entity_meta_views(&entity)?;
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        self.add_node_at_locked(
            &mut ns,
            path,
            entity,
            NodeMeta_ {
                name,
                type_id,
                default_data,
                input_defs,
                output_defs,
            },
        )
    }

    /// Atomically allocate a SubGraphNode with its two InterfaceNodes
    /// under a single `NodeStates` write lock.
    ///
    /// Thin wrapper around [`add_subgraph_at_with_id`] that mints a
    /// fresh `NodeId` for the SubGraph itself.
    pub fn add_subgraph_at(
        &mut self,
        parent_path: &NodePath,
        label: impl Into<String>,
    ) -> Result<NodeId, String> {
        self.add_subgraph_at_with_id(parent_path, NodeId::new(), label)
    }

    /// Atomically allocate a SubGraphNode with its two InterfaceNodes
    /// using the caller-provided `sg_id` (plan-007 P007b).
    ///
    /// Creates:
    /// - SubGraphNode at `parent_path.child(sg_id)`
    /// - Input-direction InterfaceNode at `sg_path.child(in_id)`
    /// - Output-direction InterfaceNode at `sg_path.child(out_id)`
    ///
    /// The two proxy ids (`in_id`, `out_id`) are still minted
    /// internally; only the SubGraph's own id is taken from the
    /// argument. Returns the same `sg_id` on success.
    ///
    /// All three insertions and both direction stamps happen while
    /// `node_states` is write-locked, so no reader can observe the
    /// SubGraph in a half-built state. On failure, any nodes already
    /// inserted within this call are rolled back.
    pub fn add_subgraph_at_with_id(
        &mut self,
        parent_path: &NodePath,
        sg_id: NodeId,
        label: impl Into<String>,
    ) -> Result<NodeId, String> {
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
                "add_subgraph_at_with_id: path collision under {}",
                parent_path
            ));
        }

        // Build all three entities before any mutation.
        let sg_entity: NodeEntity = Arc::new(std::sync::RwLock::new(SubGraphNode::with_proxies(
            in_id, out_id, label,
        )));
        let in_entity: NodeEntity = Arc::new(std::sync::RwLock::new(InterfaceNode));
        let out_entity: NodeEntity = Arc::new(std::sync::RwLock::new(InterfaceNode));

        // Acquire the NodeStates write lock ONCE for all three insertions
        // and both direction stamps.
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;

        let mut inserted: Vec<NodePath> = Vec::with_capacity(3);
        let result = (|| -> Result<(), String> {
            self.add_node_at_locked(
                &mut ns,
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

            self.add_node_at_locked(
                &mut ns,
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
            Self::stamp_interface_direction_locked(&mut ns, &in_path, InterfaceDirection::Input)?;

            self.add_node_at_locked(
                &mut ns,
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
            Self::stamp_interface_direction_locked(&mut ns, &out_path, InterfaceDirection::Output)?;

            Ok(())
        })();

        if let Err(e) = result {
            // Rollback: remove any successfully-inserted nodes (ns still held).
            for p in inserted.iter().rev() {
                let _ = self.remove_node_at_locked(&mut ns, p);
            }
            return Err(e);
        }

        Ok(sg_id)
    }

    // ── Path-aware factory dispatch (plan-007 P007b) ──

    /// Create a node by name under `parent_path`, recovering slot
    /// metadata from inventory or the runtime factory name registry.
    ///
    /// The root variant
    /// ([`NodeGraphWrite::create_node_by_name`]) is now a thin wrapper
    /// over this method with `parent_path = NodePath::root()`.
    pub fn create_node_by_name_at(
        &mut self,
        parent_path: &NodePath,
        name: &str,
    ) -> Result<NodeId, String> {
        let (node_id, default_data, type_id) = self
            .node_manager
            .create_node_by_name_at(parent_path, name)?;
        self.register_named_node_in_states(parent_path, node_id, name, default_data, type_id)?;
        Ok(node_id)
    }

    /// Create a node by name under `parent_path` using the caller-provided
    /// `id`. Path-aware sibling of
    /// [`NodeGraphWrite::create_node_by_name_with_id`].
    pub fn create_node_by_name_at_with_id(
        &mut self,
        parent_path: &NodePath,
        id: NodeId,
        name: &str,
    ) -> Result<NodeId, String> {
        let (_node_id, default_data, type_id) =
            self.node_manager
                .create_node_by_name_at_with_id(parent_path, id, name)?;
        self.register_named_node_in_states(parent_path, id, name, default_data, type_id)?;
        Ok(id)
    }

    // ── Path-aware node-data / slot-default mutators (plan-007 P007c) ──

    /// Replace the node-data payload at `path` and flag the path as
    /// changed. Path-aware sibling of
    /// [`NodeGraphWrite::update_node_data`]; the root variant is now a
    /// thin wrapper over this method.
    pub fn update_node_data_at(&mut self, path: &NodePath, data: Data) -> Result<(), String> {
        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
        let node_state = guard
            .get_mut(path)
            .ok_or_else(|| format!("Node at path {} not found", path))?;
        node_state.data = Some(data);
        guard.mark_changed(path);
        Ok(())
    }

    /// Replace the default value of input slot `slot_index` at `path`.
    /// Path-aware sibling of
    /// [`NodeGraphWrite::update_input_slot_default_data`]; the root
    /// variant is now a thin wrapper over this method.
    pub fn update_input_slot_default_data_at(
        &mut self,
        path: &NodePath,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String> {
        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
        let slot = guard
            .input_slot_mut(path, slot_index)
            .ok_or_else(|| format!("Input slot {} not found at path {}", slot_index, path))?;
        slot.default_value = Some(data.into_value());
        guard.mark_changed(path);
        Ok(())
    }

    /// Replace a single field of the node-data payload at `path`.
    /// Path-aware sibling of
    /// [`NodeGraphWrite::update_node_data_field`]; the root variant is
    /// now a thin wrapper over this method.
    pub fn update_node_data_field_at(
        &mut self,
        path: &NodePath,
        data_type: DataType,
        field_name: &str,
        value: f64,
    ) -> Result<(), String> {
        // Read current field values from existing node data
        let current_fields: Vec<(&str, f64)> = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let state = ns
                .get(path)
                .ok_or_else(|| format!("Node at path {} not found", path))?;
            extract_fields_from_data(data_type, state.data.as_ref())
        };

        // Assemble new Data by replacing the target field
        let data = data_type
            .assemble(|f| {
                if f == field_name {
                    value
                } else {
                    current_fields
                        .iter()
                        .find(|(name, _)| *name == f)
                        .map(|(_, v)| *v)
                        .unwrap_or(0.0)
                }
            })
            .ok_or_else(|| {
                format!(
                    "Cannot assemble data for type {:?} (non-composite type)",
                    data_type
                )
            })?;

        self.update_node_data_at(path, data)
    }

    /// Replace a single field of the input-slot default at `path`.
    /// Path-aware sibling of
    /// [`NodeGraphWrite::update_input_slot_default_field`]; the root
    /// variant is now a thin wrapper over this method.
    pub fn update_input_slot_default_field_at(
        &mut self,
        path: &NodePath,
        slot_index: usize,
        field_name: &str,
        value: f64,
    ) -> Result<(), String> {
        // Read current field values and data_type from existing slot default
        let (data_type, current_fields) = {
            let ns = self.node_states.read().map_err(|e| e.to_string())?;
            let slot = ns
                .input_slot(path, slot_index)
                .ok_or_else(|| format!("Input slot {} not found at path {}", slot_index, path))?;
            let dt = slot.data_type;
            let fields = extract_fields_from_default_value(dt, slot.default_value.as_ref());
            (dt, fields)
        };

        // Assemble new Data by replacing the target field
        let data = data_type
            .assemble(|f| {
                if f == field_name {
                    value
                } else {
                    current_fields
                        .iter()
                        .find(|(name, _)| *name == f)
                        .map(|(_, v)| *v)
                        .unwrap_or(0.0)
                }
            })
            .ok_or_else(|| {
                format!(
                    "Cannot assemble data for type {:?} (non-composite type)",
                    data_type
                )
            })?;

        self.update_input_slot_default_data_at(path, slot_index, data)
    }

    /// Shared NodeStates registration step for the `_at` factory
    /// dispatch paths. Resolves the static `'static` name + slot defs
    /// (from inventory when available, otherwise from the leaked
    /// runtime-registered factory name) and inserts the path entry.
    fn register_named_node_in_states(
        &mut self,
        parent_path: &NodePath,
        node_id: NodeId,
        name: &str,
        default_data: Option<Data>,
        type_id: Option<TypeId>,
    ) -> Result<(), String> {
        // SubGraphNode external slots are derived from the child
        // InterfaceNodes (transparent-container architecture) —
        // no mirroring needed at creation time.
        let (resolved_name, inputs, outputs): (&'static str, &[SlotDef], &[SlotDef]) =
            match crate::get_node_type_info(name) {
                Some(info) => (info.name, info.inputs, info.outputs),
                None => (self.node_manager.leaked_factory_name(name)?, &[], &[]),
            };

        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
        guard.add_node(
            &parent_path.child(node_id),
            resolved_name,
            type_id,
            default_data,
            inputs,
            outputs,
        );
        Ok(())
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
            ns.remove_edges_for_node_at(p);
            let _ = self.remove_node_at_locked(&mut ns, p);
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

    /// Stamp an InterfaceNode at `path` with the given direction.
    ///
    /// Acquires the `NodeStates` write lock and delegates to the
    /// `_locked` variant. Exposed as `pub(crate)` for P3c.12+ callers;
    /// the current P3c.10 codebase uses the internal `_locked` form.
    #[allow(dead_code)]
    pub(crate) fn stamp_interface_direction(
        &mut self,
        path: &NodePath,
        direction: InterfaceDirection,
    ) -> Result<(), String> {
        let mut ns = self.node_states.write().map_err(|e| e.to_string())?;
        Self::stamp_interface_direction_locked(&mut ns, path, direction)
    }
}

/// Compact metadata bundle for `add_node_at_locked`.
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
    // Read the static name before dropping the guard so the `'static`
    // lifetime is preserved naturally — no transmute needed.
    let name: &'static str = entity.read().map_err(|e| e.to_string())?.node_name();

    // For externally-supplied entities we cannot recover static slot defs
    // because the trait object erases them. The caller is responsible for
    // registering slots separately via `add_input_slot`/`add_output_slot`.
    // We just use empty slices here and rely on the caller to wire slots.
    Ok((name, None, None, &[], &[]))
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
    #[test]
    fn add_subgraph_at_root_registers_three_nodes() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "TestSG")
            .expect("add_subgraph_at");

        // SubGraphNode in NodeManager at root.child(sg_id)
        assert!(graph
            .node_at_path(&crate::NodePath::root().child(sg_id))
            .is_some());

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
        assert!(graph.node_at_path(&sg_path).is_none());
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

    #[test]
    fn stamp_interface_direction_pub_crate_wrapper_works() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let (in_id, _) = graph
            .subgraph_proxy_ids_at_path(&sg_path)
            .expect("proxy ids");
        let in_path = sg_path.child(in_id);
        // Re-stamping the same direction should succeed without error.
        assert!(graph
            .stamp_interface_direction(&in_path, crate::InterfaceDirection::Input)
            .is_ok());
    }

    // ── Path-aware API additions (plan-007 P007b) ──

    #[test]
    fn create_node_by_name_at_subgraph_registers_slots() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);

        let add_id = graph
            .create_node_by_name_at(&sg_path, "Add")
            .expect("create_node_by_name_at Add");
        let add_path = sg_path.child(add_id);

        // The entity is reachable at the depth-2 path.
        assert!(graph.node_at_path(&add_path).is_some());

        // Slot metadata was recovered from the inventory entry — the
        // load-bearing distinction vs raw `add_node_at`.
        let ns = graph.node_states.read().expect("read node_states");
        assert_eq!(
            ns.input_slot_count(&add_path),
            2,
            "Add should have 2 input slots at depth-2 path",
        );
        assert_eq!(
            ns.output_slot_count(&add_path),
            1,
            "Add should have 1 output slot at depth-2 path",
        );
    }

    #[test]
    fn add_subgraph_at_with_id_preserves_caller_id() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = crate::NodeId::new();
        let returned = graph
            .add_subgraph_at_with_id(&crate::NodePath::root(), sg_id, "SG")
            .expect("add_subgraph_at_with_id");
        assert_eq!(returned, sg_id, "must return the caller-supplied id");

        let sg_path = crate::NodePath::root().child(sg_id);
        assert!(
            graph.node_at_path(&sg_path).is_some(),
            "SubGraph must resolve at the caller-supplied path",
        );
        // Two proxy children are still created internally.
        assert_eq!(graph.children_of_path(&sg_path).len(), 2);
    }

    #[test]
    fn register_graph_factory_invokes_closure_at_parent_path() {
        use std::sync::Arc;

        let mut graph = crate::NodeGraph::new().expect("create graph");
        let captured = Arc::new(std::sync::Mutex::new(None::<crate::NodeId>));
        let captured_in_closure = Arc::clone(&captured);

        let factory: crate::GraphFactory = Arc::new(move |g, parent_path, _id_hint| {
            let id = g.create_node_by_name_at(parent_path, "Number")?;
            *captured_in_closure.lock().unwrap() = Some(id);
            Ok(id)
        });
        graph.register_graph_factory("TestFactory", factory);

        let returned = graph
            .create_graph_by_name_at(&crate::NodePath::root(), "TestFactory", None)
            .expect("create_graph_by_name_at");

        let from_closure = captured.lock().unwrap().expect("closure ran");
        assert_eq!(
            returned, from_closure,
            "dispatcher returns the factory's NodeId verbatim",
        );
        // Node exists at root.child(returned).
        assert!(graph
            .node_at_path(&crate::NodePath::root().child(returned))
            .is_some());
    }

    // ── update_*_at family tests (plan-007 P007c) ──

    /// Test-only node with a `Vector3` input slot. Built-in nodes only
    /// expose `Vector3` on *output* slots; the slot-default-field
    /// round-trip test needs an input slot of a composite type to
    /// exercise the assemble-from-fields path.
    #[derive(Debug)]
    struct Vec3InputNode;

    impl crate::NodeMeta for Vec3InputNode {
        const NAME: &'static str = "Vec3InputTestNode";
        const CATEGORY: crate::NodeCategory = crate::NodeCategory::Primitive;
        const INPUTS: &'static [crate::SlotDef] = &[crate::SlotDef {
            label: "In",
            data_type: crate::DataType::Vector3,
            max_connections: Some(1),
        }];
        const OUTPUTS: &'static [crate::SlotDef] = &[];
    }

    impl crate::NodeImpl for Vec3InputNode {
        fn execute_sync(&self, _ctx: crate::ExecutionContext) -> Result<(), String> {
            Ok(())
        }
    }

    crate::register_nodes!(Vec3InputNode);

    #[test]
    fn update_node_data_at_depth_2_marks_path_changed() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let v3_id = graph
            .create_node_by_name_at(&sg_path, "Vector3")
            .expect("create_node_by_name_at Vector3");
        let v3_path = sg_path.child(v3_id);

        // Drain any change records from creation so we observe only the
        // mutation we are about to perform.
        {
            let mut ns = graph.node_states.write().expect("write ns");
            let _ = ns.drain_changed_nodes();
        }

        let new_data =
            crate::Data::new(crate::Vector3::new(1.0, 2.0, 3.0)).expect("Data::new Vector3");
        graph
            .update_node_data_at(&v3_path, new_data)
            .expect("update_node_data_at");

        // The depth-2 path is the canonical key in changed_nodes.
        let changed = graph
            .node_states
            .read()
            .expect("read ns")
            .peek_changed_nodes();
        assert!(
            changed.contains(&v3_path),
            "depth-2 path {} must appear in changed_nodes, got {:?}",
            v3_path,
            changed
        );

        // The data round-trips through node_data_at_path.
        let data = graph.node_data_at_path(&v3_path).expect("data set");
        let v = data.value::<crate::Vector3>().expect("Vector3 payload");
        assert_eq!((v.x, v.y, v.z), (1.0, 2.0, 3.0));
    }

    #[test]
    fn update_input_slot_default_data_at_depth_2_sets_slot() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let add_id = graph
            .create_node_by_name_at(&sg_path, "Add")
            .expect("create_node_by_name_at Add");
        let add_path = sg_path.child(add_id);

        let new_default = crate::Data::new(11.5_f64).expect("Data::new f64");
        graph
            .update_input_slot_default_data_at(&add_path, 0, new_default)
            .expect("update_input_slot_default_data_at");

        let ns = graph.node_states.read().expect("read ns");
        let slot = ns
            .input_slot(&add_path, 0)
            .expect("input slot 0 must exist");
        let dv = slot
            .default_value
            .as_ref()
            .expect("default_value set after update");
        let value = dv.downcast_ref::<f64>().expect("f64 default");
        assert_eq!(*value, 11.5);
    }

    #[test]
    fn update_node_data_field_at_depth_2_field_round_trip() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let v3_id = graph
            .create_node_by_name_at(&sg_path, "Vector3")
            .expect("create_node_by_name_at Vector3");
        let v3_path = sg_path.child(v3_id);

        graph
            .update_node_data_field_at(&v3_path, crate::DataType::Vector3, "y", 7.25)
            .expect("update_node_data_field_at y");

        let data = graph.node_data_at_path(&v3_path).expect("data set");
        let v = data.value::<crate::Vector3>().expect("Vector3 payload");
        // Only the targeted field changed; defaults for x/z were 0.0.
        assert_eq!((v.x, v.y, v.z), (0.0, 7.25, 0.0));
    }

    #[test]
    fn update_input_slot_default_field_at_depth_2_round_trip() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let n_id = graph
            .create_node_by_name_at(&sg_path, "Vec3InputTestNode")
            .expect("create_node_by_name_at Vec3InputTestNode");
        let n_path = sg_path.child(n_id);

        graph
            .update_input_slot_default_field_at(&n_path, 0, "z", -4.5)
            .expect("update_input_slot_default_field_at z");

        let ns = graph.node_states.read().expect("read ns");
        let slot = ns.input_slot(&n_path, 0).expect("input slot 0");
        let dv = slot
            .default_value
            .as_ref()
            .expect("default_value set after field update");
        let v = dv
            .downcast_ref::<crate::Vector3>()
            .expect("Vector3 default value");
        assert_eq!((v.x, v.y, v.z), (0.0, 0.0, -4.5));
    }

    #[test]
    fn update_node_data_root_wrapper_still_works() {
        use crate::NodeGraphWrite;

        let mut graph = crate::NodeGraph::new().expect("create graph");
        let v3_id = graph
            .create_node_by_name("Vector3")
            .expect("create_node_by_name Vector3");

        // Mutate via the root-level trait method.
        let new_data =
            crate::Data::new(crate::Vector3::new(9.0, 8.0, 7.0)).expect("Data::new Vector3");
        graph
            .update_node_data(&v3_id, new_data)
            .expect("update_node_data root wrapper");

        // Same read works through the path-aware getter, confirming the
        // root wrapper writes to the canonical `root.child(id)` path.
        let data = graph
            .node_data_at_path(&crate::NodePath::root().child(v3_id))
            .expect("data set");
        let v = data.value::<crate::Vector3>().expect("Vector3 payload");
        assert_eq!((v.x, v.y, v.z), (9.0, 8.0, 7.0));
    }
}
