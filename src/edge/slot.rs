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

impl Data {
    pub fn value<T: Clone>(&self) -> Option<T>
    where
        T: 'static,
    {
        if let Some(value) = self.as_any().downcast_ref::<T>() {
            Some(value.clone())
        } else {
            None
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        match self {
            Data::Number(n) => n as &dyn std::any::Any,
            Data::String(s) => s as &dyn std::any::Any,
        }
    }
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
