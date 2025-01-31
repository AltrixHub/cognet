pub mod slot;
pub use slot::*;

use crate::{impl_entity_id, NodeId};

impl_entity_id!(EdgeId);

#[derive(Debug, Clone)]
pub struct Edge {
    pub from_node_id: NodeId,
    pub from_output_slot_index: usize,
    pub from_output_slot_id: OutputSlotId,
    pub to_node_id: NodeId,
    pub to_input_slot_index: usize,
    pub to_input_slot_id: InputSlotId,
}
