use std::{
    any::{type_name, Any, TypeId},
    fmt::Debug,
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

impl DataType {
    fn type_id(&self) -> TypeId {
        match self {
            DataType::Number => TypeId::of::<f64>(),
            DataType::String => TypeId::of::<String>(),
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            DataType::Number => type_name::<f64>(),
            DataType::String => type_name::<String>(),
        }
    }
}

#[derive(Debug)]
pub struct Data {
    value: Arc<dyn Any + Send + Sync>,
    data_type: DataType,
}

impl Data {
    pub fn new<T: Any + Send + Sync + Debug>(value: T) -> Result<Self, String> {
        let type_id = TypeId::of::<T>();

        let data_type = match type_id {
            t if t == TypeId::of::<f64>() => DataType::Number,
            t if t == TypeId::of::<String>() => DataType::String,
            _ => return Err(format!("Invalid DataType: {}", type_name::<T>())),
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

    pub fn value<T: 'static>(&self) -> Result<&T, String> {
        if TypeId::of::<T>() == self.data_type.type_id() {
            self.value.downcast_ref::<T>().ok_or_else(|| {
                format!(
                    "Type mismatch: Expected {}, but got {}",
                    self.data_type.type_name(),
                    type_name::<T>()
                )
            })
        } else {
            Err(format!(
                "Type mismatch: Expected {}, but got {}",
                self.data_type.type_name(),
                type_name::<T>()
            ))
        }
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
