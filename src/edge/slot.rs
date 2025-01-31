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

    fn valid_data_type_from_any(&self, value: &Arc<dyn Any + Send + Sync>) -> bool {
        match self {
            DataType::Number => value.downcast_ref::<f64>().is_some(),
            DataType::String => value.downcast_ref::<String>().is_some(),
        }
    }
}

#[derive(Debug)]
pub struct Data {
    value: Arc<dyn Any + Send + Sync>,
    data_type: DataType,
}

impl Data {
    pub fn new<T: Any + Send + Sync>(value: T) -> Result<Self, String> {
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

    pub fn from_any(value: Arc<dyn Any + Send + Sync>) -> Result<Self, String> {
        let type_id = (*value).type_id();
        let data_type = match type_id {
            t if t == TypeId::of::<f64>() => DataType::Number,
            t if t == TypeId::of::<String>() => DataType::String,
            _ => return Err("Unsupported data type".to_string()),
        };

        Ok(Data {
            value: Arc::from(value),
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
                    "Data type mismatch: Expected {}, but got {}",
                    self.data_type.type_name(),
                    type_name::<T>()
                )
            })
        } else {
            Err(format!(
                "Data type mismatch: Expected {}, but got {}",
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
    pub default_value: Option<Arc<dyn Any + Send + Sync>>,
    pub connected_edges: Vec<EdgeId>,
}

impl InputSlot {
    pub fn default_value(&self) -> &Option<Arc<dyn Any + Send + Sync>> {
        &self.default_value
    }

    pub fn set_default_value(&mut self, value: Arc<dyn Any + Send + Sync>) -> Result<(), String> {
        if !self.data_type.valid_data_type_from_any(&value) {
            return Err(format!(
                "Failed set default value due to type mismatch: expected {}",
                self.data_type.type_name(),
            ));
        }
        self.default_value = Some(value);
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct OutputSlot {
    pub id: OutputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub connected_edges: Vec<EdgeId>,
}
