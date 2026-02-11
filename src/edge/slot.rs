use std::{
    any::{type_name, Any, TypeId},
    fmt::Debug,
    sync::Arc,
};

use crate::impl_entity_id;
use serde::{Deserialize, Serialize};

impl_entity_id!(InputSlotId);
impl_entity_id!(OutputSlotId);

pub enum SlotId {
    Input(InputSlotId),
    Output(OutputSlotId),
}

#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub enum DataType {
    #[default]
    Number,
    String,
    /// Mesh data type for geometry outputs.
    /// Stores arbitrary mesh data as `Arc<dyn Any>` via `Data::from_mesh()`.
    Mesh,
}

impl DataType {
    fn type_id(&self) -> TypeId {
        match self {
            DataType::Number => TypeId::of::<f64>(),
            DataType::String => TypeId::of::<String>(),
            // Mesh holds arbitrary types; type_id is not used for Mesh (see Data::value())
            DataType::Mesh => TypeId::of::<()>(),
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            DataType::Number => type_name::<f64>(),
            DataType::String => type_name::<String>(),
            DataType::Mesh => "Mesh",
        }
    }

    fn valid_data(&self, data: &Data) -> bool {
        *self == data.data_type
    }

    fn _valid_data_value_type(&self, value: &DataValue) -> bool {
        match self {
            DataType::Number => value.downcast_ref::<f64>().is_some(),
            DataType::String => value.downcast_ref::<String>().is_some(),
            // Mesh accepts any type
            DataType::Mesh => true,
        }
    }
}

pub type DataValue = Arc<dyn Any + Send + Sync>;

#[derive(Debug, Clone)]
pub struct Data {
    value: DataValue,
    data_type: DataType,
}

impl Serialize for Data {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.data_type {
            DataType::Number => {
                let value = self
                    .value
                    .downcast_ref::<f64>()
                    .ok_or_else(|| serde::ser::Error::custom("Failed to downcast value to f64"))?;
                serializer.serialize_f64(*value)
            }
            DataType::String => {
                let value = self.value.downcast_ref::<String>().ok_or_else(|| {
                    serde::ser::Error::custom("Failed to downcast value to String")
                })?;
                serializer.serialize_str(value)
            }
            DataType::Mesh => Err(serde::ser::Error::custom(
                "Mesh data type is not serializable",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Data {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DataVisitor;

        impl<'de> serde::de::Visitor<'de> for DataVisitor {
            type Value = Data;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid data type")
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Data {
                    value: Arc::new(value),
                    data_type: DataType::Number,
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Data {
                    value: Arc::new(value.to_string()),
                    data_type: DataType::String,
                })
            }
        }

        deserializer.deserialize_any(DataVisitor)
    }
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

    /// Create a Mesh data value from any type.
    ///
    /// Mesh data holds arbitrary types as `Arc<dyn Any>`, allowing custom
    /// geometry types to flow through the graph without cognet needing
    /// to know about them.
    pub fn from_mesh<T: Any + Send + Sync + 'static>(value: T) -> Self {
        Data {
            value: Arc::new(value),
            data_type: DataType::Mesh,
        }
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
        if self.data_type == DataType::Mesh {
            // Mesh holds arbitrary types; bypass TypeId check and downcast directly
            self.value.downcast_ref::<T>().ok_or_else(|| {
                format!(
                    "Mesh data downcast failed: requested {}",
                    type_name::<T>()
                )
            })
        } else if TypeId::of::<T>() == self.data_type.type_id() {
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

#[derive(Debug, Default)]
pub struct InputSlot {
    pub id: InputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub default_value: Option<DataValue>,
    pub max_connections: Option<usize>,
}

impl InputSlot {
    pub fn default_value<T: 'static>(&self) -> Result<&T, String> {
        if TypeId::of::<T>() == self.data_type.type_id() {
            let default_value = self
                .default_value
                .as_ref()
                .ok_or_else(|| format!("Input slot doesn't have default value"))?;
            default_value.downcast_ref::<T>().ok_or_else(|| {
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

    pub fn set_default_value(&mut self, data: Data) -> Result<(), String> {
        if !self.data_type.valid_data(&data) {
            return Err(format!(
                "Failed set input slot default value due to type mismatch: expected {}",
                self.data_type.type_name(),
            ));
        }
        self.default_value = Some(data.value);
        Ok(())
    }

    pub fn max_connections(&self) -> &Option<usize> {
        &self.max_connections
    }
}

#[derive(Debug, Default)]
pub struct OutputSlot {
    pub id: OutputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
}
