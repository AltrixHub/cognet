pub mod slot;
pub use slot::*;

use crate::{impl_entity_id, NodePath};

impl_entity_id!(EdgeId);

/// A directed connection between two nodes in the graph.
///
/// Both endpoints carry a full `NodePath` (not a bare `NodeId`) so that
/// path-aware traversal in the executor and renderer does not need to
/// re-derive the path from a slot-id reverse lookup on every hop.
///
/// The slot-id fields (`from_output_slot_id`, `to_input_slot_id`) are
/// globally unique ulids that survive slot reordering; `NodeStates` is
/// the authoritative owner and keeps the slot-id ↔ `(NodePath, index)`
/// inverse maps in lock-step with these fields.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from_node: NodePath,
    pub from_output_slot_index: usize,
    pub from_output_slot_id: OutputSlotId,
    pub to_node: NodePath,
    pub to_input_slot_index: usize,
    pub to_input_slot_id: InputSlotId,
}
