use crate::{EdgeId, TypedId};

pub type InputSlotId = TypedId<InputSlot>;
pub type OutputSlotId = TypedId<OutputSlot>;

pub enum SlotId {
    Input(InputSlotId),
    Output(OutputSlotId),
}

#[derive(Debug, PartialEq, Default, Clone)]
pub enum DataType {
    #[default]
    Number,
    String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Number(f32),
    String(String),
}

#[derive(Debug, Default, Clone)]
pub struct InputSlot {
    pub id: InputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub connected_edges: Vec<EdgeId>,
}

#[derive(Debug, Default, Clone)]
pub struct OutputSlot {
    pub id: OutputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub connected_edges: Vec<EdgeId>,
}
