pub mod slot;
pub use slot::*;

use crate::NodeId;

#[derive(Debug)]
pub struct Edge {
    pub from_node: NodeId,
    pub from_slot: OutputSlotId,
    pub to_node: NodeId,
    pub to_slot: InputSlotId,
}
