use ulid::Ulid;

use crate::EdgeId;

pub type InputSlotId = Ulid;
pub type OutputSlotId = Ulid;

pub enum SlotId {
    Input(InputSlotId),
    Output(OutputSlotId),
}

#[derive(Debug)]
pub struct InputSlot {
    pub id: InputSlotId,
    pub index: usize,
    pub label: &'static str,
    pub data_type: String,
    pub connected_edges: Vec<EdgeId>,
}

#[derive(Debug)]
pub struct OutputSlot {
    pub id: OutputSlotId,
    pub index: usize,
    pub label: &'static str,
    pub data_type: String,
    pub connected_edges: Vec<EdgeId>,
}
