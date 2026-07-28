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
    Data, DataType, InputSlotId, InputSlotInfo, InterfaceDirection, InterfaceNode,
    InterfaceNodeData, NodeEntity, NodeGraph, NodeId, NodeMeta, NodePath, OutputSlotId, SlotDef,
    SlotInfo, SubGraphNode, INTERFACE_NODE_DATA_DOMAIN,
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

    // ── Path-aware slot / type queries ──
    //
    // Path-aware siblings of the root-only NodeId-keyed accessors in
    // `node_graph/mod.rs`. The root variants are now thin wrappers
    // over these.

    /// Get the type name of the node at `path`. Path-aware sibling of
    /// [`NodeGraph::node_type_name`]; the root variant is a thin
    /// wrapper over this method.
    pub fn node_type_name_at(&self, path: &NodePath) -> Option<&'static str> {
        let ns = self.node_states.read().ok()?;
        ns.get(path).map(|s| s.type_name)
    }

    /// Get the input slot count of the node at `path`. Path-aware
    /// sibling of [`NodeGraph::input_slot_count`]; the root variant is
    /// a thin wrapper over this method.
    pub fn input_slot_count_at(&self, path: &NodePath) -> usize {
        self.node_states
            .read()
            .ok()
            .map(|ns| ns.input_slot_count(path))
            .unwrap_or(0)
    }

    /// Get the output slot count of the node at `path`. Path-aware
    /// sibling of [`NodeGraph::output_slot_count`]; the root variant
    /// is a thin wrapper over this method.
    pub fn output_slot_count_at(&self, path: &NodePath) -> usize {
        self.node_states
            .read()
            .ok()
            .map(|ns| ns.output_slot_count(path))
            .unwrap_or(0)
    }

    /// Get combined info for an input slot at `path`. Path-aware
    /// sibling of [`NodeGraph::input_slot_info`]; the root variant is
    /// a thin wrapper over this method.
    pub fn input_slot_info_at(&self, path: &NodePath, slot: usize) -> Option<InputSlotInfo> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(path, slot).map(|s| InputSlotInfo {
            id: s.id,
            label: s.label,
            data_type: s.data_type,
            default_value: s.default_value.as_ref().map(Arc::clone),
        })
    }

    /// Get combined info for an output slot at `path`. Path-aware
    /// sibling of [`NodeGraph::output_slot_info`]; the root variant is
    /// a thin wrapper over this method.
    pub fn output_slot_info_at(&self, path: &NodePath, slot: usize) -> Option<SlotInfo> {
        let ns = self.node_states.read().ok()?;
        ns.output_slot(path, slot).map(|s| SlotInfo {
            label: s.label,
            data_type: s.data_type,
        })
    }

    /// Get the data type of an input slot at `path`. Path-aware
    /// sibling of [`NodeGraph::input_slot_data_type`]; the root
    /// variant is a thin wrapper over this method.
    pub fn input_slot_data_type_at(&self, path: &NodePath, slot: usize) -> Option<DataType> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(path, slot).map(|s| s.data_type)
    }

    /// Get the **ordered** edge list connected to an input slot at
    /// `path`. Path-aware sibling of
    /// [`NodeGraph::edges_for_input_slot`]; the root variant is a thin
    /// wrapper over this method. The order is the slot's connection
    /// order (semantic for variable-length inputs such as polyline
    /// points), matching [`NodeStates::edges_for_input`].
    pub fn edges_for_input_slot_at(&self, path: &NodePath, slot: usize) -> Vec<crate::EdgeId> {
        self.node_states
            .read()
            .ok()
            .and_then(|ns| {
                let slot_state = ns.input_slot(path, slot)?;
                Some(ns.edges_for_input(&slot_state.id).to_vec())
            })
            .unwrap_or_default()
    }

    /// Get the **ordered** edge list connected from an output slot at
    /// `path`. Path-aware sibling of
    /// [`NodeGraph::edges_for_output_slot`]; the root variant is a
    /// thin wrapper over this method.
    pub fn edges_for_output_slot_at(&self, path: &NodePath, slot: usize) -> Vec<crate::EdgeId> {
        self.node_states
            .read()
            .ok()
            .and_then(|ns| {
                let slot_state = ns.output_slot(path, slot)?;
                Some(ns.edges_for_output(&slot_state.id).to_vec())
            })
            .unwrap_or_default()
    }

    /// Whether an input slot should be rendered as an editable row by
    /// downstream property inspectors. Returns `None` when the slot does
    /// not exist; callers may treat that as visible.
    pub fn input_slot_inspector_visible_at(&self, path: &NodePath, slot: usize) -> Option<bool> {
        let ns = self.node_states.read().ok()?;
        ns.input_slot(path, slot).map(|s| s.inspector_visible)
    }

    /// Override the `inspector_visible` flag of an existing input slot.
    /// Returns `true` on success, `false` if the slot does not exist.
    /// Used by callers that create dynamic SubGraph ports and want to
    /// mark a subset of them as structural (hidden from Inspectors).
    pub fn set_input_slot_inspector_visible_at(
        &self,
        path: &NodePath,
        slot: usize,
        visible: bool,
    ) -> bool {
        let Ok(mut ns) = self.node_states.write() else {
            return false;
        };
        ns.set_input_slot_inspector_visible(path, slot, visible)
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

    /// Clear the node-data payload at `path` (reset it to `None`) and
    /// flag the path as changed. Companion to
    /// [`update_node_data_at`](Self::update_node_data_at) with the same
    /// path resolution and dirty contract, so a cleared node re-executes
    /// exactly like an updated one.
    ///
    /// Added for op-log inverse support: the inverse of setting node data
    /// over an absent prior payload must reproduce absence, which
    /// `update_node_data_at` cannot express (its `Data` argument is
    /// non-optional).
    ///
    /// Clearing an already-absent payload is a no-op `Ok` — inverse
    /// sequences are replayed blindly, so the clear is idempotent. A
    /// missing node is still a typed `Err` (same contract as the setter).
    pub fn clear_node_data_at(&mut self, path: &NodePath) -> Result<(), String> {
        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;
        let node_state = guard
            .get_mut(path)
            .ok_or_else(|| format!("Node at path {} not found", path))?;
        node_state.data = None;
        guard.mark_changed(path);
        Ok(())
    }

    /// Replace the default value of input slot `slot_index` at `path`.
    /// Path-aware sibling of
    /// [`NodeGraphWrite::update_input_slot_default_data`]; the root
    /// variant is now a thin wrapper over this method.
    ///
    /// plan-010 §3.7: when `path` resolves to a `SubGraphNode`, the
    /// default is also mirrored onto the SubGraph's `InputProxy` input
    /// slot so internals can read the value at execute time. Non-SubGraph
    /// paths skip the mirror. Atomic: both target slots are pre-validated
    /// before any mutation. Both paths are marked changed so dirty
    /// planning re-runs the proxy and downstream internals.
    pub fn update_input_slot_default_data_at(
        &mut self,
        path: &NodePath,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String> {
        // Resolve proxy WITHOUT mutating; None = path is not a SubGraph,
        // so the mirror is skipped and we behave like the legacy
        // single-slot updater.
        let proxy_path = self
            .subgraph_proxy_ids_at_path(path)
            .map(|(input_proxy_id, _)| path.child(input_proxy_id));

        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;

        // Pre-validate ALL targets before mutating (atomicity invariant).
        if guard.input_slot(path, slot_index).is_none() {
            return Err(format!(
                "update_input_slot_default_data_at: slot {} not found at path {}",
                slot_index, path
            ));
        }
        if let Some(ref ppath) = proxy_path {
            if guard.input_slot(ppath, slot_index).is_none() {
                return Err(format!(
                    "update_input_slot_default_data_at: InputProxy missing slot {} \
                     for SubGraph at {}",
                    slot_index, path
                ));
            }
        }

        // Atomic mutate.
        let value = data.into_value();
        guard
            .input_slot_mut(path, slot_index)
            .expect("validated above")
            .default_value = Some(Arc::clone(&value));
        if let Some(ref ppath) = proxy_path {
            guard
                .input_slot_mut(ppath, slot_index)
                .expect("validated above")
                .default_value = Some(value);
        }

        // Dirty BOTH paths — see plan-010 §3.7.
        guard.mark_changed(path);
        if let Some(ppath) = proxy_path {
            guard.mark_changed(&ppath);
        }
        Ok(())
    }

    /// Clear the default value of input slot `slot_index` at `path`
    /// (reset it to `None`). Companion to
    /// [`update_input_slot_default_data_at`](Self::update_input_slot_default_data_at)
    /// with the same SubGraph-proxy mirror, atomicity, and dirty
    /// contract. Added for op-log inverse support: the inverse of
    /// setting a value over an absent prior default must reproduce
    /// absence.
    ///
    /// Clearing an already-absent default is a no-op `Ok` — inverse
    /// sequences are replayed blindly, so the clear is idempotent. A
    /// missing slot is still a typed `Err`.
    pub fn clear_input_slot_default_data_at(
        &mut self,
        path: &NodePath,
        slot_index: usize,
    ) -> Result<(), String> {
        // Resolve proxy WITHOUT mutating; None = path is not a SubGraph,
        // so the mirror is skipped (same shape as the update sibling).
        let proxy_path = self
            .subgraph_proxy_ids_at_path(path)
            .map(|(input_proxy_id, _)| path.child(input_proxy_id));

        let mut guard = self.node_states.write().map_err(|e| e.to_string())?;

        // Pre-validate ALL targets before mutating (atomicity invariant).
        if guard.input_slot(path, slot_index).is_none() {
            return Err(format!(
                "clear_input_slot_default_data_at: slot {} not found at path {}",
                slot_index, path
            ));
        }
        if let Some(ref ppath) = proxy_path {
            if guard.input_slot(ppath, slot_index).is_none() {
                return Err(format!(
                    "clear_input_slot_default_data_at: InputProxy missing slot {} \
                     for SubGraph at {}",
                    slot_index, path
                ));
            }
        }

        // Atomic mutate.
        guard
            .input_slot_mut(path, slot_index)
            .expect("validated above")
            .default_value = None;
        if let Some(ref ppath) = proxy_path {
            guard
                .input_slot_mut(ppath, slot_index)
                .expect("validated above")
                .default_value = None;
        }

        // Dirty BOTH paths — see plan-010 §3.7.
        guard.mark_changed(path);
        if let Some(ppath) = proxy_path {
            guard.mark_changed(&ppath);
        }
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
            <crate::AddNode as crate::NodeMeta>::INPUTS.len(),
            "Add's inputs should be recovered from the inventory entry",
        );
        assert_eq!(
            ns.output_slot_count(&add_path),
            <crate::AddNode as crate::NodeMeta>::OUTPUTS.len(),
            "Add's outputs should be recovered from the inventory entry",
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
            inspector_visible: true,
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
    fn clear_node_data_at_depth_2_removes_data_and_marks_path_changed() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let v3_id = graph
            .create_node_by_name_at(&sg_path, "Vector3")
            .expect("create_node_by_name_at Vector3");
        let v3_path = sg_path.child(v3_id);

        let data = crate::Data::new(crate::Vector3::new(1.0, 2.0, 3.0)).expect("Data::new Vector3");
        graph
            .update_node_data_at(&v3_path, data)
            .expect("update_node_data_at");

        // Drain the change records from creation + the update so we
        // observe only the clear.
        {
            let mut ns = graph.node_states.write().expect("write ns");
            let _ = ns.drain_changed_nodes();
        }

        graph
            .clear_node_data_at(&v3_path)
            .expect("clear_node_data_at");

        // Same dirty contract as update: the depth-2 path is the
        // canonical key in changed_nodes, so the cleared node re-executes.
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

        assert!(
            graph.node_data_at_path(&v3_path).is_none(),
            "node_data must be absent after clear_node_data_at",
        );
    }

    /// Inverse sequences are replayed blindly, so clearing an
    /// already-absent payload must be an idempotent `Ok`.
    #[test]
    fn clear_node_data_at_is_idempotent_when_data_already_absent() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let v3_id = graph
            .create_node_by_name_at(&crate::NodePath::root(), "Vector3")
            .expect("create_node_by_name_at Vector3");
        let v3_path = crate::NodePath::root().child(v3_id);

        graph
            .clear_node_data_at(&v3_path)
            .expect("first clear_node_data_at");
        graph
            .clear_node_data_at(&v3_path)
            .expect("clearing an already-absent payload is a no-op Ok");

        assert!(graph.node_data_at_path(&v3_path).is_none());
    }

    /// A missing node is a typed `Err`, not a silent no-op — same
    /// contract as `update_node_data_at`.
    #[test]
    fn clear_node_data_at_errors_on_unknown_path() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let missing = crate::NodePath::root().child(crate::NodeId::new());

        let err = graph
            .clear_node_data_at(&missing)
            .expect_err("unknown path must be an Err");
        assert!(
            err.contains("not found"),
            "error must name the missing node, got {err}",
        );
    }

    /// plan-010 §3.7: when the path resolves to a SubGraph node, the
    /// default is mirrored onto the InputProxy's input slot too.
    #[test]
    fn update_input_slot_default_data_at_mirrors_to_proxy_when_target_is_subgraph() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_input(&sg_id, "x", crate::DataType::Number)
            .expect("add input");

        let sg_path = crate::NodePath::root().child(sg_id);
        graph
            .update_input_slot_default_data_at(
                &sg_path,
                0,
                crate::Data::new(13.0_f64).expect("Data::new"),
            )
            .expect("update default");

        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let proxy_path = sg_path.child(input_proxy_id);
        let ns = graph.node_states.read().expect("read");
        assert!(
            ns.input_slot(&sg_path, 0).unwrap().default_value.is_some(),
            "external default written",
        );
        assert!(
            ns.input_slot(&proxy_path, 0)
                .unwrap()
                .default_value
                .is_some(),
            "proxy default mirrored",
        );
    }

    /// plan-010 §3.7: atomic mutate — if the proxy slot is missing
    /// (schema corruption), neither slot must be updated.
    #[test]
    fn update_input_slot_default_data_at_leaves_no_partial_state_on_proxy_slot_missing() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_input(&sg_id, "x", crate::DataType::Number)
            .expect("add input");

        let sg_path = crate::NodePath::root().child(sg_id);
        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let proxy_path = sg_path.child(input_proxy_id);

        // Simulate schema corruption — remove the proxy input slot.
        {
            let mut ns = graph.node_states.write().expect("write");
            ns.remove_input_slot(&proxy_path, 0);
        }

        let err = graph
            .update_input_slot_default_data_at(
                &sg_path,
                0,
                crate::Data::new(13.0_f64).expect("Data::new"),
            )
            .expect_err("mirror should Err on missing proxy slot");
        assert!(
            err.contains("InputProxy"),
            "error mentions InputProxy: {err}"
        );

        let ns = graph.node_states.read().expect("read");
        assert!(
            ns.input_slot(&sg_path, 0).unwrap().default_value.is_none(),
            "external slot must not be mutated when mirror fails",
        );
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

    // ── clear_input_slot_default_data_at (op-log inverse support) ──

    /// set → clear → the slot's default is `None` again and the path is
    /// marked changed. This is the plain-node absence-restoring inverse
    /// the app's op-log needs for `SetInputValue` over an absent prior.
    #[test]
    fn clear_input_slot_default_data_at_resets_default_to_none() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let add_id = graph
            .create_node_by_name_at(&crate::NodePath::root(), "Add")
            .expect("create Add");
        let add_path = crate::NodePath::root().child(add_id);

        graph
            .update_input_slot_default_data_at(
                &add_path,
                0,
                crate::Data::new(4.0_f64).expect("Data::new"),
            )
            .expect("seed default");
        // Drain the change set so the clear's own dirty mark is visible.
        {
            let mut ns = graph.node_states.write().expect("write ns");
            let _ = ns.drain_changed_nodes();
        }

        graph
            .clear_input_slot_default_data_at(&add_path, 0)
            .expect("clear default");

        let ns = graph.node_states.read().expect("read ns");
        assert!(
            ns.input_slot(&add_path, 0).unwrap().default_value.is_none(),
            "default must be None after clear",
        );
        assert!(
            ns.peek_changed_nodes().contains(&add_path),
            "clear must mark the path changed",
        );
    }

    /// Clearing an already-absent default is a no-op `Ok` — the op-log
    /// replays inverse sequences blindly, so an idempotent clear must
    /// not error.
    #[test]
    fn clear_input_slot_default_data_at_over_absent_is_noop_ok() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let add_id = graph
            .create_node_by_name_at(&crate::NodePath::root(), "Add")
            .expect("create Add");
        let add_path = crate::NodePath::root().child(add_id);

        graph
            .clear_input_slot_default_data_at(&add_path, 0)
            .expect("clear over absent default must be Ok");

        let ns = graph.node_states.read().expect("read ns");
        assert!(
            ns.input_slot(&add_path, 0).unwrap().default_value.is_none(),
            "default stays None",
        );
    }

    /// Missing slot → typed Err (mirrors the update sibling).
    #[test]
    fn clear_input_slot_default_data_at_errs_on_missing_slot() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let add_id = graph
            .create_node_by_name_at(&crate::NodePath::root(), "Add")
            .expect("create Add");
        let add_path = crate::NodePath::root().child(add_id);

        let err = graph
            .clear_input_slot_default_data_at(&add_path, 99)
            .expect_err("slot 99 does not exist");
        assert!(err.contains("slot 99"), "error names the slot: {err}");
    }

    /// When the path resolves to a SubGraph node, the clear is mirrored
    /// onto the InputProxy's input slot too (same contract as the
    /// update sibling, plan-010 §3.7).
    #[test]
    fn clear_input_slot_default_data_at_mirrors_to_proxy_when_target_is_subgraph() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        graph
            .add_subgraph_input(&sg_id, "x", crate::DataType::Number)
            .expect("add input");
        let sg_path = crate::NodePath::root().child(sg_id);
        graph
            .update_input_slot_default_data_at(
                &sg_path,
                0,
                crate::Data::new(13.0_f64).expect("Data::new"),
            )
            .expect("seed default");

        graph
            .clear_input_slot_default_data_at(&sg_path, 0)
            .expect("clear default");

        let (input_proxy_id, _) = graph.subgraph_proxy_ids(&sg_id).expect("proxy ids");
        let proxy_path = sg_path.child(input_proxy_id);
        let ns = graph.node_states.read().expect("read");
        assert!(
            ns.input_slot(&sg_path, 0).unwrap().default_value.is_none(),
            "external default cleared",
        );
        assert!(
            ns.input_slot(&proxy_path, 0)
                .unwrap()
                .default_value
                .is_none(),
            "proxy default cleared too",
        );
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

    // ── Path-aware slot / type read APIs ──

    /// Build a graph with a SubGraph containing an `Add` node and
    /// return `(graph, add_path)`.
    fn graph_with_subgraph_add_node() -> (crate::NodeGraph, crate::NodePath) {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let add_id = graph
            .create_node_by_name_at(&sg_path, "Add")
            .expect("create_node_by_name_at Add");
        let add_path = sg_path.child(add_id);
        (graph, add_path)
    }

    #[test]
    fn node_type_name_at_depth_2_returns_inner_type() {
        let (graph, add_path) = graph_with_subgraph_add_node();
        assert_eq!(
            graph.node_type_name_at(&add_path),
            Some("Add"),
            "node_type_name_at must resolve the inner node type at depth-2",
        );
    }

    #[test]
    fn input_slot_count_at_depth_2_returns_inner_count() {
        let (graph, add_path) = graph_with_subgraph_add_node();
        assert_eq!(
            graph.input_slot_count_at(&add_path),
            <crate::AddNode as crate::NodeMeta>::INPUTS.len(),
            "the depth-2 path reports the inner Add's own input count",
        );
    }

    #[test]
    fn output_slot_count_at_depth_2_returns_inner_count() {
        let (graph, add_path) = graph_with_subgraph_add_node();
        assert_eq!(
            graph.output_slot_count_at(&add_path),
            <crate::AddNode as crate::NodeMeta>::OUTPUTS.len(),
            "the depth-2 path reports the inner Add's own output count",
        );
    }

    #[test]
    fn input_slot_info_at_depth_2_returns_label() {
        let (graph, add_path) = graph_with_subgraph_add_node();
        let info = graph
            .input_slot_info_at(&add_path, 0)
            .expect("input slot 0 info");
        assert_eq!(info.label, "A");
        assert_eq!(info.data_type, crate::DataType::Number);
    }

    #[test]
    fn output_slot_info_at_depth_2_returns_label() {
        let (graph, add_path) = graph_with_subgraph_add_node();
        let info = graph
            .output_slot_info_at(&add_path, 0)
            .expect("output slot 0 info");
        assert_eq!(info.label, "Sum");
        assert_eq!(info.data_type, crate::DataType::Number);
    }

    #[test]
    fn input_slot_data_type_at_depth_2_returns_type() {
        let (graph, add_path) = graph_with_subgraph_add_node();
        assert_eq!(
            graph.input_slot_data_type_at(&add_path, 0),
            Some(crate::DataType::Number),
        );
    }

    #[test]
    fn edges_for_input_slot_at_depth_2_returns_connect_order() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let add_id = graph
            .create_node_by_name_at(&sg_path, "AddList")
            .expect("create AddList");
        let n1 = graph
            .create_node_by_name_at(&sg_path, "Number")
            .expect("create Number 1");
        let n2 = graph
            .create_node_by_name_at(&sg_path, "Number")
            .expect("create Number 2");
        let add_path = sg_path.child(add_id);
        let e1 = graph
            .connect_nodes_at(&sg_path.child(n1), 0, &add_path, 0)
            .expect("connect n1");
        let e2 = graph
            .connect_nodes_at(&sg_path.child(n2), 0, &add_path, 0)
            .expect("connect n2");

        assert_eq!(
            graph.edges_for_input_slot_at(&add_path, 0),
            vec![e1, e2],
            "path-aware input-slot edge list must preserve connect order at depth-2",
        );
    }

    #[test]
    fn edges_for_output_slot_at_depth_2_returns_connect_order() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg_id = graph
            .add_subgraph_at(&crate::NodePath::root(), "SG")
            .expect("add_subgraph_at");
        let sg_path = crate::NodePath::root().child(sg_id);
        let num_id = graph
            .create_node_by_name_at(&sg_path, "Number")
            .expect("create Number");
        let a1 = graph
            .create_node_by_name_at(&sg_path, "Add")
            .expect("create Add 1");
        let a2 = graph
            .create_node_by_name_at(&sg_path, "Add")
            .expect("create Add 2");
        let num_path = sg_path.child(num_id);
        let e1 = graph
            .connect_nodes_at(&num_path, 0, &sg_path.child(a1), 0)
            .expect("connect a1");
        let e2 = graph
            .connect_nodes_at(&num_path, 0, &sg_path.child(a2), 0)
            .expect("connect a2");

        assert_eq!(
            graph.edges_for_output_slot_at(&num_path, 0),
            vec![e1, e2],
            "path-aware output-slot edge list must preserve connect order at depth-2",
        );
    }

    #[test]
    fn node_type_name_root_wrapper_still_works() {
        use crate::NodeGraphWrite;

        let mut graph = crate::NodeGraph::new().expect("create graph");
        let add_id = graph
            .create_node_by_name("Add")
            .expect("create_node_by_name Add");

        // Root-level NodeId-keyed accessor must still report the
        // correct type name after refactoring to a thin path wrapper.
        assert_eq!(graph.node_type_name(&add_id), Some("Add"));

        // And the path-aware read at the canonical root path agrees.
        assert_eq!(
            graph.node_type_name_at(&crate::NodePath::root().child(add_id)),
            Some("Add"),
        );
    }
}
