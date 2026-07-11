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

/// A one-dimensional numeric interval `t0 ..= t1` — the first-class
/// "Domain" of parametric workflows (curve parameters, remapping).
/// `t0 > t1` (decreasing) is allowed; consumers that need an ordered
/// span use [`Interval::min`] / [`Interval::max`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub t0: f64,
    pub t1: f64,
}

impl Interval {
    pub fn new(t0: f64, t1: f64) -> Self {
        Self { t0, t1 }
    }

    /// Signed length (`t1 - t0`).
    pub fn length(&self) -> f64 {
        self.t1 - self.t0
    }

    pub fn min(&self) -> f64 {
        self.t0.min(self.t1)
    }

    pub fn max(&self) -> f64 {
        self.t0.max(self.t1)
    }

    /// Whether `t` lies within the interval (inclusive, order-agnostic).
    pub fn contains(&self, t: f64) -> bool {
        t >= self.min() && t <= self.max()
    }

    /// Map `t` from this interval into `target`, preserving the
    /// normalized position. A zero-length source maps everything to
    /// `target.t0`.
    pub fn remap(&self, t: f64, target: &Interval) -> f64 {
        let len = self.length();
        if len.abs() < f64::EPSILON {
            return target.t0;
        }
        let normalized = (t - self.t0) / len;
        target.t0 + normalized * target.length()
    }
}

/// An oriented coordinate frame: origin plus right-handed orthonormal
/// axes — the positional currency of plane-based construction and
/// `Orient`-style placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub origin: Vector3,
    pub x_axis: Vector3,
    pub y_axis: Vector3,
    pub z_axis: Vector3,
}

impl Plane {
    /// Build a plane from origin + X/Y axes, deriving Z = X × Y.
    ///
    /// Validates that both axes are unit length and orthogonal within
    /// `1e-9` — a skewed frame must never silently shear geometry.
    pub fn new(origin: Vector3, x_axis: Vector3, y_axis: Vector3) -> Result<Self, String> {
        const EPS: f64 = 1e-9;
        let len = |v: &Vector3| (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
        if (len(&x_axis) - 1.0).abs() > EPS || (len(&y_axis) - 1.0).abs() > EPS {
            return Err("Plane: axes must be unit length".to_string());
        }
        let dot = x_axis.x * y_axis.x + x_axis.y * y_axis.y + x_axis.z * y_axis.z;
        if dot.abs() > EPS {
            return Err("Plane: axes must be orthogonal".to_string());
        }
        let z_axis = Vector3::new(
            x_axis.y * y_axis.z - x_axis.z * y_axis.y,
            x_axis.z * y_axis.x - x_axis.x * y_axis.z,
            x_axis.x * y_axis.y - x_axis.y * y_axis.x,
        );
        Ok(Self {
            origin,
            x_axis,
            y_axis,
            z_axis,
        })
    }

    /// World XY plane at `origin`.
    pub fn xy(origin: Vector3) -> Self {
        Self {
            origin,
            x_axis: Vector3::new(1.0, 0.0, 0.0),
            y_axis: Vector3::new(0.0, 1.0, 0.0),
            z_axis: Vector3::new(0.0, 0.0, 1.0),
        }
    }

    /// Plane from origin + normal (Z axis). X/Y are derived with a
    /// stable convention: X = normalize(world-up × Z) unless Z is
    /// nearly vertical, in which case world-X seeds the frame.
    ///
    /// Errors on a zero-length normal.
    pub fn from_normal(origin: Vector3, normal: Vector3) -> Result<Self, String> {
        let len = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
        if len < 1e-12 {
            return Err("Plane: normal must have non-zero length".to_string());
        }
        let z = Vector3::new(normal.x / len, normal.y / len, normal.z / len);
        let seed = if z.z.abs() > 0.999 {
            Vector3::new(1.0, 0.0, 0.0)
        } else {
            Vector3::new(0.0, 0.0, 1.0)
        };
        // X = normalize(seed × Z), Y = Z × X.
        let x = Vector3::new(
            seed.y * z.z - seed.z * z.y,
            seed.z * z.x - seed.x * z.z,
            seed.x * z.y - seed.y * z.x,
        );
        let xl = (x.x * x.x + x.y * x.y + x.z * x.z).sqrt();
        let x = Vector3::new(x.x / xl, x.y / xl, x.z / xl);
        let y = Vector3::new(
            z.y * x.z - z.z * x.y,
            z.z * x.x - z.x * x.z,
            z.x * x.y - z.y * x.x,
        );
        Ok(Self {
            origin,
            x_axis: x,
            y_axis: y,
            z_axis: z,
        })
    }
}

/// A 4x4 affine transform (row-major), the reusable product of
/// placement operations: compose, invert, and apply to geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Row-major matrix entries; row 3 is `[0, 0, 0, 1]` for affine
    /// transforms.
    pub m: [[f64; 4]; 4],
}

impl Transform {
    #[rustfmt::skip]
    pub fn identity() -> Self {
        Self { m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]}
    }

    /// The rigid transform taking `source` onto `target`:
    /// `p → o_t + R (p - o_s)` with `R = B_t · B_sᵀ`.
    pub fn from_frames(source: &Plane, target: &Plane) -> Self {
        let s = [source.x_axis, source.y_axis, source.z_axis];
        let t = [target.x_axis, target.y_axis, target.z_axis];
        let axis = |v: Vector3, i: usize| match i {
            0 => v.x,
            1 => v.y,
            _ => v.z,
        };
        let mut m = Self::identity().m;
        for (i, row) in m.iter_mut().take(3).enumerate() {
            for (j, cell) in row.iter_mut().take(3).enumerate() {
                let mut r = 0.0;
                for k in 0..3 {
                    r += axis(t[k], i) * axis(s[k], j);
                }
                *cell = r;
            }
        }
        let o_s = [source.origin.x, source.origin.y, source.origin.z];
        let o_t = [target.origin.x, target.origin.y, target.origin.z];
        for (row, ot) in m.iter_mut().zip(o_t) {
            let mut r_os = 0.0;
            for (j, os) in o_s.iter().enumerate() {
                r_os += row[j] * os;
            }
            row[3] = ot - r_os;
        }
        Self { m }
    }

    /// `self` followed by `next` (`next · self`).
    pub fn then(&self, next: &Transform) -> Self {
        let mut m = [[0.0; 4]; 4];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                for k in 0..4 {
                    *cell += next.m[i][k] * self.m[k][j];
                }
            }
        }
        Self { m }
    }

    /// Affine inverse (3x3 adjugate + translation).
    ///
    /// # Errors
    ///
    /// `Err` when the linear part is singular (determinant ~ 0).
    pub fn inverse(&self) -> Result<Self, String> {
        let a = &self.m;
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        if det.abs() < 1e-12 {
            return Err("Transform: singular linear part cannot be inverted".to_string());
        }
        let inv_det = 1.0 / det;
        let mut inv = Self::identity().m;
        inv[0][0] = (a[1][1] * a[2][2] - a[1][2] * a[2][1]) * inv_det;
        inv[0][1] = (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * inv_det;
        inv[0][2] = (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * inv_det;
        inv[1][0] = (a[1][2] * a[2][0] - a[1][0] * a[2][2]) * inv_det;
        inv[1][1] = (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * inv_det;
        inv[1][2] = (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * inv_det;
        inv[2][0] = (a[1][0] * a[2][1] - a[1][1] * a[2][0]) * inv_det;
        inv[2][1] = (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * inv_det;
        inv[2][2] = (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * inv_det;
        for row in inv.iter_mut().take(3) {
            let mut t = 0.0;
            for (j, a_row) in a.iter().take(3).enumerate() {
                t += row[j] * a_row[3];
            }
            row[3] = -t;
        }
        Ok(Self { m: inv })
    }

    /// Apply to a point.
    pub fn apply_point(&self, p: Vector3) -> Vector3 {
        let m = &self.m;
        Vector3::new(
            m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
            m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
            m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
        )
    }
}

/// Element kind of a homogeneous, flat [`DataType::List`].
///
/// Nested lists are deliberately unrepresentable (decided 2026-07-10):
/// the engine stays "one execution per node, one value per wire" —
/// iteration lives inside nodes, and there are no GH-style data trees.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ListElem {
    Number,
    String,
    Bool,
    Vector3,
    Color,
    Interval,
    Plane,
}

/// Types that can be list elements (maps a Rust payload type to its
/// [`ListElem`] tag for `Data::from_list`).
pub trait ListElement: std::any::Any + Send + Sync {
    const ELEM: ListElem;
}

impl ListElement for f64 {
    const ELEM: ListElem = ListElem::Number;
}
impl ListElement for String {
    const ELEM: ListElem = ListElem::String;
}
impl ListElement for bool {
    const ELEM: ListElem = ListElem::Bool;
}
impl ListElement for Vector3 {
    const ELEM: ListElem = ListElem::Vector3;
}
impl ListElement for ColorValue {
    const ELEM: ListElem = ListElem::Color;
}
impl ListElement for Interval {
    const ELEM: ListElem = ListElem::Interval;
}
impl ListElement for Plane {
    const ELEM: ListElem = ListElem::Plane;
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
    /// A one-dimensional numeric interval (`t0 ..= t1`).
    Interval,
    /// An oriented coordinate frame (origin + orthonormal axes).
    Plane,
    /// A 4x4 affine transform.
    Transform,
    /// A homogeneous, flat list of `ListElem` values carried as one
    /// `Data` on the wire (`Vec<T>` payload).
    List(ListElem),
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
            DataType::Interval => "Interval",
            DataType::Plane => "Plane",
            DataType::Transform => "Transform",
            DataType::List(elem) => match elem {
                ListElem::Number => "List<Number>",
                ListElem::String => "List<String>",
                ListElem::Bool => "List<Bool>",
                ListElem::Vector3 => "List<Vector3>",
                ListElem::Color => "List<Color>",
                ListElem::Interval => "List<Interval>",
                ListElem::Plane => "List<Plane>",
            },
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
            DataType::Interval,
            DataType::Plane,
            DataType::List(ListElem::Number),
            DataType::List(ListElem::Vector3),
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
            DataType::Interval => &["t0", "t1"],
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
            DataType::Interval => Data::new(Interval::new(read_field("t0"), read_field("t1"))).ok(),
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
            DataType::Interval => TypeId::of::<Interval>(),
            DataType::Plane => TypeId::of::<Plane>(),
            DataType::Transform => TypeId::of::<Transform>(),
            DataType::List(elem) => match elem {
                ListElem::Number => TypeId::of::<Vec<f64>>(),
                ListElem::String => TypeId::of::<Vec<String>>(),
                ListElem::Bool => TypeId::of::<Vec<bool>>(),
                ListElem::Vector3 => TypeId::of::<Vec<Vector3>>(),
                ListElem::Color => TypeId::of::<Vec<ColorValue>>(),
                ListElem::Interval => TypeId::of::<Vec<Interval>>(),
                ListElem::Plane => TypeId::of::<Vec<Plane>>(),
            },
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
            DataType::Interval => "Interval",
            DataType::Plane => "Plane",
            DataType::Transform => "Transform",
            DataType::List(_) => "List",
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
            DataType::Interval => Err(serde::ser::Error::custom(
                "Interval data type is not serializable",
            )),
            DataType::Plane => Err(serde::ser::Error::custom(
                "Plane data type is not serializable",
            )),
            DataType::Transform => Err(serde::ser::Error::custom(
                "Transform data type is not serializable",
            )),
            DataType::List(_) => Err(serde::ser::Error::custom(
                "List data types are not serializable",
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
            t if t == TypeId::of::<Interval>() => DataType::Interval,
            t if t == TypeId::of::<Plane>() => DataType::Plane,
            t if t == TypeId::of::<Transform>() => DataType::Transform,
            t if t == TypeId::of::<Vec<f64>>() => DataType::List(ListElem::Number),
            t if t == TypeId::of::<Vec<String>>() => DataType::List(ListElem::String),
            t if t == TypeId::of::<Vec<bool>>() => DataType::List(ListElem::Bool),
            t if t == TypeId::of::<Vec<Vector3>>() => DataType::List(ListElem::Vector3),
            t if t == TypeId::of::<Vec<ColorValue>>() => DataType::List(ListElem::Color),
            t if t == TypeId::of::<Vec<Interval>>() => DataType::List(ListElem::Interval),
            t if t == TypeId::of::<Vec<Plane>>() => DataType::List(ListElem::Plane),
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

    /// Create a homogeneous list value (`DataType::List(T::ELEM)`).
    pub fn from_list<T: ListElement>(values: Vec<T>) -> Self {
        Data {
            value: Arc::new(values),
            data_type: DataType::List(T::ELEM),
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
            t if t == TypeId::of::<Interval>() => DataType::Interval,
            t if t == TypeId::of::<Plane>() => DataType::Plane,
            t if t == TypeId::of::<Vec<f64>>() => DataType::List(ListElem::Number),
            t if t == TypeId::of::<Vec<Vector3>>() => DataType::List(ListElem::Vector3),
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

    #[test]
    fn interval_data_roundtrip_and_type_inference() {
        let data = Data::new(Interval::new(2.0, 6.0)).unwrap();
        assert_eq!(data.get_type(), DataType::Interval);
        let interval = data.value::<Interval>().unwrap();
        assert!((interval.length() - 4.0).abs() < 1e-12);
    }

    #[test]
    fn interval_remap_preserves_normalized_position() {
        let source = Interval::new(0.0, 10.0);
        let target = Interval::new(100.0, 200.0);
        assert!((source.remap(2.5, &target) - 125.0).abs() < 1e-12);
        // Decreasing target flips direction.
        let flipped = Interval::new(1.0, 0.0);
        assert!((source.remap(2.5, &flipped) - 0.75).abs() < 1e-12);
        // Zero-length source collapses to target start.
        let zero = Interval::new(3.0, 3.0);
        assert!((zero.remap(3.0, &target) - 100.0).abs() < 1e-12);
    }

    #[test]
    fn interval_contains_is_order_agnostic() {
        assert!(Interval::new(5.0, 1.0).contains(2.0));
        assert!(!Interval::new(5.0, 1.0).contains(6.0));
    }

    #[test]
    fn interval_assembles_from_fields() {
        let dt = DataType::Interval;
        assert_eq!(dt.field_names(), &["t0", "t1"]);
        let data = dt.assemble(|f| if f == "t0" { 1.0 } else { 4.0 }).unwrap();
        let interval = data.value::<Interval>().unwrap();
        assert!((interval.t0 - 1.0).abs() < 1e-12 && (interval.t1 - 4.0).abs() < 1e-12);
    }

    #[test]
    fn plane_data_roundtrip_and_validation() {
        let plane = Plane::new(
            Vector3::new(1.0, 2.0, 3.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        assert!((plane.z_axis.z - 1.0).abs() < 1e-12);
        let data = Data::new(plane).unwrap();
        assert_eq!(data.get_type(), DataType::Plane);
        assert!(data.value::<Plane>().is_ok());

        // Non-unit / non-orthogonal axes are rejected.
        assert!(Plane::new(
            Vector3::zero(),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        )
        .is_err());
        assert!(Plane::new(
            Vector3::zero(),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .is_err());
    }

    #[test]
    fn plane_from_normal_builds_an_orthonormal_frame() {
        let plane = Plane::from_normal(Vector3::zero(), Vector3::new(0.0, 0.0, 2.0)).unwrap();
        let dot_xy = plane.x_axis.x * plane.y_axis.x
            + plane.x_axis.y * plane.y_axis.y
            + plane.x_axis.z * plane.y_axis.z;
        assert!(dot_xy.abs() < 1e-9);
        assert!((plane.z_axis.z - 1.0).abs() < 1e-9);
        assert!(Plane::from_normal(Vector3::zero(), Vector3::zero()).is_err());
    }

    #[test]
    fn list_data_is_typed_by_element() {
        let numbers = Data::from_list(vec![1.0, 2.0, 3.0]);
        assert_eq!(numbers.get_type(), DataType::List(ListElem::Number));
        assert_eq!(numbers.value::<Vec<f64>>().unwrap().len(), 3);
        // Element-typed read is enforced.
        assert!(numbers.value::<Vec<bool>>().is_err());

        let points = Data::new(vec![Vector3::zero()]).unwrap();
        assert_eq!(points.get_type(), DataType::List(ListElem::Vector3));

        // Different element kinds are different wire types.
        assert_ne!(
            DataType::List(ListElem::Number),
            DataType::List(ListElem::Vector3)
        );
    }

    #[test]
    fn transform_compose_invert_roundtrip() {
        let source = Plane::xy(Vector3::zero());
        let target = Plane::new(
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
        )
        .unwrap();
        let t = Transform::from_frames(&source, &target);
        let p = t.apply_point(Vector3::new(1.0, 0.0, 0.0));
        // 90° CCW rotation + translate (5,0,0): (1,0,0) → (5,1,0).
        assert!((p.x - 5.0).abs() < 1e-12 && (p.y - 1.0).abs() < 1e-12);

        let inv = t.inverse().unwrap();
        let back = inv.apply_point(p);
        assert!((back.x - 1.0).abs() < 1e-12 && back.y.abs() < 1e-12);

        let ident = t.then(&inv);
        let q = ident.apply_point(Vector3::new(3.0, -2.0, 7.0));
        assert!(
            (q.x - 3.0).abs() < 1e-12 && (q.y + 2.0).abs() < 1e-12 && (q.z - 7.0).abs() < 1e-12
        );

        let data = Data::new(t).unwrap();
        assert_eq!(data.get_type(), DataType::Transform);
    }
}
