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

/// A 3D vector value for passing spatial data through the graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

/// An RGBA color value for passing color data through the graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorValue {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl ColorValue {
    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }
}

/// A collection of 3D vertices with an open/closed flag.
#[derive(Debug, Clone)]
pub struct Vertices {
    pub points: Vec<Vector3>,
    pub closed: bool,
}

impl Vertices {
    pub fn new(points: Vec<Vector3>, closed: bool) -> Self {
        Self { points, closed }
    }
}

#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub enum DataType {
    #[default]
    Number,
    String,
    /// Mesh data type for geometry outputs.
    /// Stores arbitrary mesh data as `Arc<dyn Any>` via `Data::from_mesh()`.
    Mesh,
    /// 3D vector (x, y, z).
    Vector3,
    /// RGBA color value.
    Color,
    /// A collection of vertices with an open/closed flag.
    Vertices,
    /// A boolean value.
    Bool,
}

impl DataType {
    fn type_id(&self) -> TypeId {
        match self {
            DataType::Number => TypeId::of::<f64>(),
            DataType::String => TypeId::of::<String>(),
            // Mesh holds arbitrary types; type_id is not used for Mesh (see Data::value())
            DataType::Mesh => TypeId::of::<()>(),
            DataType::Vector3 => TypeId::of::<Vector3>(),
            DataType::Color => TypeId::of::<ColorValue>(),
            DataType::Vertices => TypeId::of::<Vertices>(),
            DataType::Bool => TypeId::of::<bool>(),
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            DataType::Number => type_name::<f64>(),
            DataType::String => type_name::<String>(),
            DataType::Mesh => "Mesh",
            DataType::Vector3 => "Vector3",
            DataType::Color => "Color",
            DataType::Vertices => "Vertices",
            DataType::Bool => "Bool",
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
            DataType::Vector3 => value.downcast_ref::<Vector3>().is_some(),
            DataType::Color => value.downcast_ref::<ColorValue>().is_some(),
            DataType::Vertices => value.downcast_ref::<Vertices>().is_some(),
            DataType::Bool => value.downcast_ref::<bool>().is_some(),
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
            DataType::Vector3 => Err(serde::ser::Error::custom(
                "Vector3 data type is not serializable",
            )),
            DataType::Color => Err(serde::ser::Error::custom(
                "Color data type is not serializable",
            )),
            DataType::Vertices => Err(serde::ser::Error::custom(
                "Vertices data type is not serializable",
            )),
            DataType::Bool => {
                let value = self
                    .value
                    .downcast_ref::<bool>()
                    .ok_or_else(|| serde::ser::Error::custom("Failed to downcast value to bool"))?;
                serializer.serialize_bool(*value)
            }
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

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Data {
                    value: Arc::new(value),
                    data_type: DataType::Bool,
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
            t if t == TypeId::of::<Vector3>() => DataType::Vector3,
            t if t == TypeId::of::<ColorValue>() => DataType::Color,
            t if t == TypeId::of::<Vertices>() => DataType::Vertices,
            t if t == TypeId::of::<bool>() => DataType::Bool,
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
            t if t == TypeId::of::<Vector3>() => DataType::Vector3,
            t if t == TypeId::of::<ColorValue>() => DataType::Color,
            t if t == TypeId::of::<Vertices>() => DataType::Vertices,
            t if t == TypeId::of::<bool>() => DataType::Bool,
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
