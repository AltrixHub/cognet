use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use crate::{EdgeId, EntityId};

pub type InputSlotId = EntityId<InputSlot>;
pub type OutputSlotId = EntityId<OutputSlot>;

pub enum SlotId {
    Input(InputSlotId),
    Output(OutputSlotId),
}

#[derive(Debug, PartialEq, Default, Clone, Copy)]
pub enum DataType {
    #[default]
    Number,
    String,
}

#[derive(Debug)]
pub struct Data {
    value: Arc<dyn Any + Send + Sync>,
    data_type: DataType,
}

impl Data {
    pub fn new<T: Any + Send + Sync>(value: T) -> Result<Self, &'static str> {
        let type_id = TypeId::of::<T>();

        let data_type = if type_id == TypeId::of::<f64>() {
            DataType::Number
        } else if type_id == TypeId::of::<String>() {
            DataType::String
        } else {
            return Err("Invalid data type");
        };

        Ok(Data {
            value: Arc::new(value),
            data_type,
        })
    }

    pub fn share(&self) -> Self {
        Data {
            value: Arc::clone(&self.value),
            data_type: self.data_type,
        }
    }

    pub fn value<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }

    pub fn get_type(&self) -> DataType {
        self.data_type
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
