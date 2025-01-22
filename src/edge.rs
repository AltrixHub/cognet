pub mod slot;
pub use slot::*;

use crate::NodeId;

#[derive(Debug)]
pub struct Edge {
    pub from_node: NodeId,
    pub from_output_slot_index: usize,
    pub to_node: NodeId,
    pub to_input_slot_index: usize,
}
