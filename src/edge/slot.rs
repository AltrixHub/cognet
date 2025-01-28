use std::sync::Arc;

use crate::{EdgeId, EntityId};

pub type InputSlotId = EntityId<InputSlot>;
pub type OutputSlotId = EntityId<OutputSlot>;

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

#[derive(Debug)]
pub struct SharedData(Arc<Data>);

impl SharedData {
    pub fn new(data: Data) -> Self {
        SharedData(Arc::new(data))
    }

    pub fn share(&self) -> Self {
        SharedData(Arc::clone(&self.0))
    }

    pub fn get(&self) -> &Data {
        &self.0
    }

    pub fn value<T: 'static>(&self) -> Option<&T> {
        self.get().value()
    }
}

impl Data {
    pub fn value<T: 'static>(&self) -> Option<&T> {
        if let Some(value) = self.as_any().downcast_ref::<T>() {
            Some(value)
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
