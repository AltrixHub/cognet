use std::{
    any::{type_name, Any, TypeId},
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
///
/// Channels are normalized to the **0.0..=1.0 range** (graphics
/// convention, matching WGPU / OpenGL / sRGB-float). Producers must
/// emit values in this range; consumers that need 8-bit channels
/// should multiply by `255.0` at the boundary. The range is not
/// runtime-enforced — keeping the contract is the producer's
/// responsibility.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorValue {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl ColorValue {
    /// Construct a `ColorValue`. `r`, `g`, `b`, `a` are expected to be
    /// in the 0.0..=1.0 range (see struct docs). Out-of-range values
    /// are passed through without clamping so callers that intentionally
    /// over- or under-saturate (e.g. HDR producers) still work.
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
    /// BRep solid geometry.
    /// Stores arbitrary BRep data as `Arc<dyn Any>` via `Data::from_brep()`.
    BRep,
    /// Application-defined domain type.
    /// Connection validation uses name equality — only ports with the same
    /// domain name can be connected.
    Domain(&'static str),
}

impl DataType {
    /// Human-readable label for this data type.
    pub fn label(&self) -> &str {
        match self {
            DataType::Number => "Number",
            DataType::String => "String",
            DataType::Mesh => "Mesh",
            DataType::Vector3 => "Vector3",
            DataType::Color => "Color",
            DataType::Vertices => "Vertices",
            DataType::Bool => "Bool",
            DataType::BRep => "BRep",
            DataType::Domain(name) => name,
        }
    }

    /// All standard (non-Domain) data types for UI dropdowns.
    pub fn standard_types() -> &'static [DataType] {
        &[
            DataType::Number,
            DataType::String,
            DataType::Bool,
            DataType::Vector3,
            DataType::Color,
            DataType::Mesh,
            DataType::Vertices,
            DataType::BRep,
        ]
    }

    /// Field names for composite types. Primitive types return an empty slice.
    pub fn field_names(&self) -> &'static [&'static str] {
        match self {
            DataType::Vector3 => &["x", "y", "z"],
            DataType::Color => &["r", "g", "b", "a"],
            _ => &[],
        }
    }

    /// Assemble a `Data` value from individual field values.
    /// Returns `None` for primitive (non-composite) types.
    pub fn assemble(&self, read_field: impl Fn(&str) -> f64) -> Option<Data> {
        match self {
            DataType::Vector3 => Data::new(Vector3::new(
                read_field("x"),
                read_field("y"),
                read_field("z"),
            ))
            .ok(),
            DataType::Color => Data::new(ColorValue::new(
                read_field("r"),
                read_field("g"),
                read_field("b"),
                read_field("a"),
            ))
            .ok(),
            _ => None,
        }
    }

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
            DataType::BRep => TypeId::of::<()>(),
            DataType::Domain(_) => TypeId::of::<()>(),
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
            DataType::BRep => "BRep",
            DataType::Domain(name) => name,
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
            DataType::BRep => Err(serde::ser::Error::custom(
                "BRep data type is not serializable",
            )),
            DataType::Domain(name) => Err(serde::ser::Error::custom(format!(
                "Domain type '{}' is not serializable",
                name
            ))),
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

    /// Create a BRep data value from any type.
    ///
    /// BRep data holds arbitrary BRep geometry types as `Arc<dyn Any>`.
    pub fn from_brep<T: Any + Send + Sync + 'static>(value: T) -> Self {
        Data {
            value: Arc::new(value),
            data_type: DataType::BRep,
        }
    }

    /// Create a domain-typed data value.
    ///
    /// Domain types carry an application-defined name for type-safe connections.
    /// Only ports with the same domain name can be connected.
    pub fn from_domain<T: Any + Send + Sync + 'static>(value: T, name: &'static str) -> Self {
        Data {
            value: Arc::new(value),
            data_type: DataType::Domain(name),
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

        Ok(Data { value, data_type })
    }

    /// Restore a `Data` from a raw `Arc<dyn Any>` plus its known
    /// `DataType`. Use this on the snapshot-replay / slot-default
    /// preservation path where the caller already knows the slot's
    /// declared type but the payload has been type-erased into `Arc`.
    ///
    /// Unlike [`Data::from_any`] (which only handles primitives by
    /// inspecting the `TypeId`), this variant **trusts the caller's
    /// `data_type`** and is the canonical way to round-trip `Mesh`,
    /// `BRep`, and `Domain(_)` payloads — the type information was
    /// always present at the source slot, so requiring restoration to
    /// rediscover it from the erased `Arc` is impossible without a
    /// registry. Pass the type alongside the value instead.
    ///
    /// For primitive `DataType`s the call still validates that the
    /// inner `TypeId` matches.
    pub fn from_any_typed(
        value: Arc<dyn Any + Send + Sync>,
        data_type: DataType,
    ) -> Result<Self, String> {
        let inner_type_id = (*value).type_id();
        match data_type {
            DataType::Mesh | DataType::BRep | DataType::Domain(_) => {
                // The wrapping types accept arbitrary inner types — the
                // domain name (or Mesh/BRep marker) lives on `data_type`
                // and survives because the caller passed it in.
                Ok(Data { value, data_type })
            }
            _ if inner_type_id == data_type.type_id() => Ok(Data { value, data_type }),
            _ => Err(format!(
                "from_any_typed: inner type does not match declared {}",
                data_type.type_name()
            )),
        }
    }

    pub fn share(&self) -> Self {
        Data {
            value: Arc::clone(&self.value),
            data_type: self.data_type,
        }
    }

    /// Consume this Data and return the inner `DataValue` (`Arc<dyn Any + Send + Sync>`).
    pub fn into_value(self) -> DataValue {
        self.value
    }

    pub fn value<T: 'static>(&self) -> Result<&T, String> {
        match self.data_type {
            // Mesh, BRep, Domain hold arbitrary types; bypass TypeId check and downcast directly
            DataType::Mesh | DataType::BRep | DataType::Domain(_) => {
                self.value.downcast_ref::<T>().ok_or_else(|| {
                    format!(
                        "{} data downcast failed: requested {}",
                        self.data_type.type_name(),
                        type_name::<T>()
                    )
                })
            }
            _ if TypeId::of::<T>() == self.data_type.type_id() => {
                self.value.downcast_ref::<T>().ok_or_else(|| {
                    format!(
                        "Data type mismatch: Expected {}, but got {}",
                        self.data_type.type_name(),
                        type_name::<T>()
                    )
                })
            }
            _ => Err(format!(
                "Data type mismatch: Expected {}, but got {}",
                self.data_type.type_name(),
                type_name::<T>()
            )),
        }
    }

    pub fn get_type(&self) -> DataType {
        self.data_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct ProbeDomain(u32);

    #[test]
    fn from_any_typed_round_trips_domain_value() {
        let original = ProbeDomain(42);
        let d = Data::from_domain(original.clone(), "ProbeDomain");
        let raw: Arc<dyn Any + Send + Sync> = d.share().into_value();

        let restored = Data::from_any_typed(raw, DataType::Domain("ProbeDomain"))
            .expect("restoration must succeed for Domain");
        assert_eq!(restored.get_type(), DataType::Domain("ProbeDomain"));
        let downcast: &ProbeDomain = restored.value().expect("downcast back to ProbeDomain");
        assert_eq!(downcast, &original);
    }

    #[test]
    fn from_any_typed_round_trips_mesh_value() {
        let original = ProbeDomain(7);
        let d = Data::from_mesh(original.clone());
        let raw: Arc<dyn Any + Send + Sync> = d.share().into_value();

        let restored = Data::from_any_typed(raw, DataType::Mesh).expect("restoration");
        assert_eq!(restored.get_type(), DataType::Mesh);
        let downcast: &ProbeDomain = restored.value().expect("downcast");
        assert_eq!(downcast, &original);
    }

    #[test]
    fn from_any_typed_validates_primitive_type_id() {
        let d = Data::new(2.5_f64).expect("Number Data");
        let raw: Arc<dyn Any + Send + Sync> = d.share().into_value();

        // Correct primitive type passes.
        let ok = Data::from_any_typed(Arc::clone(&raw), DataType::Number).expect("ok");
        assert_eq!(ok.get_type(), DataType::Number);

        // Lying about the type fails.
        let err = Data::from_any_typed(raw, DataType::String).expect_err("type mismatch");
        assert!(err.contains("does not match declared"), "got: {err}");
    }
}
