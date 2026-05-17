//! `InterfaceNode` — generic dynamic-port node used as the
//! externally-facing I/O surface of a `SubGraphNode`.
//!
//! plan-005 Task 1. Replaces the original `SubGraphInputNode` /
//! `SubGraphOutputNode` proxies with a single unified node type whose
//! ports are managed via the [`crate::NodeGraph`] APIs
//! `add_interface_port` / `remove_interface_port` /
//! `rename_interface_port` (Task 2).
//!
//! ## Port shape
//!
//! Each port is **bidirectional**: it occupies one input slot AND one
//! output slot at the same index, with the same label and `DataType`.
//! Adding / removing / renaming a port mutates both sides atomically.
//!
//! ## Execution
//!
//! `execute_sync` is a no-op. InterfaceNode is a schema-only boundary
//! marker; the executor's `resolve_value_through_interface`
//! follow-through delivers values from external sources directly to
//! internal consumers, so no runtime tee is needed.
//!
//! ## Direction + locked set
//!
//! [`InterfaceNodeData`] stamped on the node carries the `direction`
//! (`Input` / `Output`, used by `SubGraphNode` to decide which proxy
//! represents the SubGraph's external input vs output surface) and a
//! `locked` set of port labels that cannot be removed or renamed by
//! user-driven dispatch. Both are immutable after the relevant entry
//! point sets them — direction at `SubGraphNode::new`, locked-per-port
//! at `add_interface_port(... locked = true)` time.

use std::collections::HashSet;

use crate::{
    register_nodes, DefaultValue, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

/// Direction stamp on an [`InterfaceNode`] instance.
///
/// A `SubGraphNode` instantiates exactly two `InterfaceNode`s inside —
/// one stamped `Input`, one stamped `Output`. The first becomes the
/// external input surface (values flow OUT of the SubGraph into the
/// inner graph via this node's outputs); the second is the external
/// output surface (internal results flow INTO this node's inputs and
/// out of the SubGraph via the same indices on its outputs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InterfaceDirection {
    #[default]
    Input,
    Output,
}

/// `NodeData` payload for an [`InterfaceNode`]. Travels as
/// `Data::from_domain(_, "InterfaceNodeData")`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InterfaceNodeData {
    pub direction: InterfaceDirection,
    /// Port labels that cannot be removed or renamed by user dispatch.
    /// Locked status is set when a port is added via the `_locked` API
    /// surface and removed only when the port itself is removed.
    pub locked: HashSet<String>,
}

/// Domain name used for [`crate::Data::from_domain`] /
/// [`crate::Data::from_any_typed`] round-trips of
/// [`InterfaceNodeData`].
pub const INTERFACE_NODE_DATA_DOMAIN: &str = "InterfaceNodeData";

/// Schema-only SubGraph boundary marker. InterfaceNode never writes to
/// its output cache at runtime — consumers of its output slots resolve
/// their value via the executor's `resolve_value_through_interface`
/// follow-through, which looks through the InterfaceNode chain to the
/// real source. The struct exists purely as the schema-carrier for
/// the SubGraph's external port surface (port shape, default values,
/// locked flags).
///
/// `INPUTS` and `OUTPUTS` are intentionally empty — port shape is
/// managed at runtime via the `NodeGraph::add_interface_port` family.
#[derive(Debug)]
pub struct InterfaceNode;

impl NodeMeta for InterfaceNode {
    const NAME: &'static str = "InterfaceNode";
    const CATEGORY: NodeCategory = NodeCategory::Interface;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::None;
}

impl NodeImpl for InterfaceNode {
    fn execute_sync(&self, _ctx: ExecutionContext) -> Result<(), String> {
        // Option D: InterfaceNode is a schema-only boundary. The
        // executor's `build_execution_context` follows edges back
        // through InterfaceNode boundaries (see
        // `resolve_value_through_interface`), so the runtime tee is
        // unnecessary — writing to outputs here would only duplicate
        // a value the consumers already resolve directly.
        Ok(())
    }
}

register_nodes!(InterfaceNode);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_meta_is_interface_category_with_no_static_slots() {
        assert_eq!(InterfaceNode::NAME, "InterfaceNode");
        assert!(matches!(InterfaceNode::CATEGORY, NodeCategory::Interface));
        assert!(InterfaceNode::INPUTS.is_empty());
        assert!(InterfaceNode::OUTPUTS.is_empty());
    }

    #[test]
    fn interface_direction_default_is_input() {
        assert_eq!(InterfaceDirection::default(), InterfaceDirection::Input);
    }

    #[test]
    fn interface_node_data_default_has_input_direction_and_empty_locked() {
        let d = InterfaceNodeData::default();
        assert_eq!(d.direction, InterfaceDirection::Input);
        assert!(d.locked.is_empty());
    }
}
