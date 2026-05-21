//! SubGraph node system.
//!
//! A SubGraphNode is a transparent containment boundary. Its children
//! live in the parent `NodeStates` at paths `sg_path.child(child_id)`.
//! The two `InterfaceNode` instances at `sg_path.child(input_proxy_id)`
//! and `sg_path.child(output_proxy_id)` ARE the SubGraph's external
//! schema — there are no mirrored slots on the SubGraphNode itself.
//!
//! Removed fields: `internal_graph`, `input_proxy_counts`,
//! `dynamic_input_targets`, `inject_inputs`, `execute_internal_sync`,
//! `collect_outputs`, `sync_dynamic_inputs`.

use std::any::TypeId;
use std::sync::Arc;

use crate::{
    DataType, ExecutionContext, NodeCategory, NodeCore, NodeExecutionKind, NodeGraph, NodeId,
    NodeImpl, NodeManager, NodeMeta, NodePath, SlotDef,
};

/// A transparent containment boundary for a sub-graph of nodes.
///
/// In the transparent-container architecture, the SubGraphNode holds no
/// execution state of its own. Its children
/// (including the two boundary `InterfaceNode`s) live in the parent
/// `NodeStates` at paths `parent_path.child(sg_id).child(child_id)`.
/// Execution visits all children directly — the SubGraphNode itself
/// is a no-op during cooking.
pub struct SubGraphNode {
    /// NodeId of the Input-direction InterfaceNode at
    /// `parent_path.child(sg_id).child(input_proxy_id)`.
    pub(crate) input_proxy_id: NodeId,
    /// NodeId of the Output-direction InterfaceNode at
    /// `parent_path.child(sg_id).child(output_proxy_id)`.
    pub(crate) output_proxy_id: NodeId,
    /// Display label (e.g., "Wall", "Stair").
    label: String,
    /// TypeId of the template that built this subgraph.
    template_type_id: Option<TypeId>,
}

impl std::fmt::Debug for SubGraphNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubGraphNode")
            .field("input_proxy_id", &self.input_proxy_id)
            .field("output_proxy_id", &self.output_proxy_id)
            .field("label", &self.label)
            .finish()
    }
}

impl SubGraphNode {
    /// Create a new SubGraphNode with the given proxy IDs and label.
    ///
    /// Used by `NodeGraph::add_subgraph_at` for atomic allocation.
    pub(crate) fn with_proxies(
        input_proxy_id: NodeId,
        output_proxy_id: NodeId,
        label: impl Into<String>,
    ) -> Self {
        Self {
            input_proxy_id,
            output_proxy_id,
            label: label.into(),
            template_type_id: None,
        }
    }

    /// Get the NodeId of the input proxy InterfaceNode.
    pub fn input_proxy_id(&self) -> NodeId {
        self.input_proxy_id
    }

    /// Get the NodeId of the output proxy InterfaceNode.
    pub fn output_proxy_id(&self) -> NodeId {
        self.output_proxy_id
    }

    /// Get the display label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Set the display label.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    /// Get the template TypeId.
    pub fn template_type_id(&self) -> Option<TypeId> {
        self.template_type_id
    }

    /// Set the template TypeId for type-safe identification.
    pub fn set_template_type_id(&mut self, type_id: TypeId) {
        self.template_type_id = Some(type_id);
    }

    /// Add a multi-connection input port (C10: multi-edge slot, not proxy fan-out).
    ///
    /// Creates one input slot on the proxy accepting up to `max_connections`
    /// edges, plus one corresponding output slot. Wall's Baseline port is
    /// the canonical user (`max_connections = None` for unbounded, or
    /// `Some(N)` for a fixed maximum).
    pub fn add_input_multi(
        &mut self,
        sg_path: &NodePath,
        graph: &mut NodeGraph,
        label: &'static str,
        data_type: DataType,
        max_connections: Option<usize>,
        inspector_visible: bool,
    ) -> Result<(), String> {
        let in_path = sg_path.child(self.input_proxy_id);
        let mut ns = graph.node_states().write().map_err(|e| e.to_string())?;
        ns.add_input_slot(
            &in_path,
            label,
            data_type,
            max_connections,
            inspector_visible,
        );
        ns.add_output_slot(&in_path, label, data_type);
        Ok(())
    }
}

impl NodeMeta for SubGraphNode {
    const NAME: &'static str = "SubGraph";
    const CATEGORY: NodeCategory = NodeCategory::Utility;
    // Dynamic slots — not used for SubGraphNode
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[];
}

impl NodeImpl for SubGraphNode {
    /// SubGraphNode is a transparent boundary — no-op during cooking.
    /// The executor visits its children (InterfaceNodes + internal nodes)
    /// directly; no special-casing is required at this node.
    fn execute_sync(&self, _ctx: ExecutionContext) -> Result<(), String> {
        Ok(())
    }

    fn execution_kind(&self) -> NodeExecutionKind {
        NodeExecutionKind::SyncCpu
    }
}

// Manual implementation of NodeCore (can't use register_nodes! with dynamic slots)
impl NodeCore for SubGraphNode {
    fn node_name(&self) -> &'static str {
        "SubGraph"
    }

    fn register_in(manager: &mut NodeManager) -> Result<(), String> {
        // Register a factory that creates a bare SubGraphNode with placeholder
        // proxy IDs. The real SubGraph allocation goes through
        // `NodeGraph::add_subgraph_at` which assigns proxy IDs atomically.
        let factory: Arc<dyn Fn() -> Result<crate::NodeEntity, String> + Send + Sync> =
            Arc::new(|| {
                let node = SubGraphNode {
                    input_proxy_id: NodeId::new(),
                    output_proxy_id: NodeId::new(),
                    label: "SubGraph".to_string(),
                    template_type_id: None,
                };
                Ok(Arc::new(std::sync::RwLock::new(node)) as crate::NodeEntity)
            });
        manager.register_factory::<SubGraphNode>(factory.clone())?;
        manager.register_factory_with_name(
            "SubGraph",
            factory,
            None,
            Some(std::any::TypeId::of::<SubGraphNode>()),
        );
        Ok(())
    }
}

// Register the SubGraphNode factory
inventory::submit! {
    crate::NodeRegistrationEntry {
        register: SubGraphNode::register_in,
    }
}

inventory::submit! {
    crate::NodeTypeInfo {
        name: "SubGraph",
        category: NodeCategory::Utility,
        inputs: &[],
        outputs: &[],
        default_value: crate::DefaultValue::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_subgraph_creation_via_add_subgraph_at() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "Test")
            .expect("Failed to create subgraph");

        // SubGraphNode exists in NodeManager
        let node = graph
            .node_at_path(&NodePath::root().child(sg_id))
            .expect("Node not found");
        let read = node.read().expect("Lock failed");
        assert_eq!(read.node_name(), "SubGraph");
        drop(read);

        // Parent NodeStates has the SubGraphNode at root level (no external slots yet)
        let ns = graph.node_states().read().expect("NodeStates lock");
        assert_eq!(ns.input_slot_count(&NodePath::root().child(sg_id)), 0);
        assert_eq!(ns.output_slot_count(&NodePath::root().child(sg_id)), 0);
    }

    #[test]
    fn test_subgraph_proxy_nodes_registered_in_parent_states() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");
        let sg_id = graph
            .add_subgraph_at(&NodePath::root(), "Test")
            .expect("Failed to create subgraph");

        let sg_path = NodePath::root().child(sg_id);
        let (input_proxy, output_proxy) = graph
            .subgraph_proxy_ids_at_path(&sg_path)
            .expect("proxy ids");

        // Both proxy nodes should be accessible via node_at_path
        let in_path = sg_path.child(input_proxy);
        let out_path = sg_path.child(output_proxy);
        assert!(graph.node_at_path(&in_path).is_some());
        assert!(graph.node_at_path(&out_path).is_some());
    }
}
