//! Static metadata for node types.
//!
//! This module provides compile-time metadata about node types,
//! allowing UI to access node information without async locks.

use crate::{ColorValue, Data, DataType, Vector3};
use std::collections::HashMap;

/// Category for organizing nodes in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeCategory {
    /// Primitive value nodes (Number, String, etc.)
    Primitive,
    /// Mathematical operations (Add, Subtract, Multiply, etc.)
    Math,
    /// Output/display nodes
    Output,
    /// Logic operations (And, Or, Not, etc.)
    Logic,
    /// String manipulation
    Text,
    /// Control flow (If, Switch, etc.)
    Control,
    /// Input/Output operations
    IO,
    /// Utility nodes
    Utility,
}

impl NodeCategory {
    /// Get the display name for this category.
    pub const fn as_str(&self) -> &'static str {
        match self {
            NodeCategory::Primitive => "Primitive",
            NodeCategory::Math => "Math",
            NodeCategory::Output => "Output",
            NodeCategory::Logic => "Logic",
            NodeCategory::Text => "Text",
            NodeCategory::Control => "Control",
            NodeCategory::IO => "IO",
            NodeCategory::Utility => "Utility",
        }
    }
}

/// Default value for a node, representable as a const.
#[derive(Debug, Clone, Copy)]
pub enum DefaultValue {
    /// No default value.
    None,
    /// A numeric default value.
    Number(f64),
    /// A string default value.
    String(&'static str),
    /// A 3D vector default value.
    Vector3 { x: f64, y: f64, z: f64 },
    /// An RGBA color default value.
    Color { r: f64, g: f64, b: f64, a: f64 },
    /// A boolean default value.
    Bool(bool),
}

impl DefaultValue {
    /// Return the `DataType` corresponding to this default value.
    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Self::None => None,
            Self::Number(_) => Some(DataType::Number),
            Self::String(_) => Some(DataType::String),
            Self::Vector3 { .. } => Some(DataType::Vector3),
            Self::Color { .. } => Some(DataType::Color),
            Self::Bool(_) => Some(DataType::Bool),
        }
    }

    /// Convert to Option<Data> at runtime.
    pub fn to_data(self) -> Option<Data> {
        match self {
            DefaultValue::None => None,
            DefaultValue::Number(n) => Data::new(n).ok(),
            DefaultValue::String(s) => Data::new(s.to_string()).ok(),
            DefaultValue::Vector3 { x, y, z } => Data::new(Vector3::new(x, y, z)).ok(),
            DefaultValue::Color { r, g, b, a } => Data::new(ColorValue::new(r, g, b, a)).ok(),
            DefaultValue::Bool(b) => Data::new(b).ok(),
        }
    }
}

/// Static definition of a slot (input or output port).
#[derive(Debug, Clone, Copy)]
pub struct SlotDef {
    pub label: &'static str,
    pub data_type: DataType,
    pub max_connections: Option<usize>,
}

/// Trait for nodes to provide static metadata.
///
/// Implement this trait to define compile-time metadata for a node type.
/// The `register_nodes!` macro will automatically use this to register
/// `NodeTypeInfo` in the inventory.
pub trait NodeMeta {
    const NAME: &'static str;
    const CATEGORY: NodeCategory;
    const INPUTS: &'static [SlotDef];
    const OUTPUTS: &'static [SlotDef];
    /// Default value for the node (optional).
    /// Use `DefaultValue::Number(10.0)` or `DefaultValue::String("hello")` for nodes with initial data.
    const DEFAULT_VALUE: DefaultValue = DefaultValue::None;
}

/// Static metadata about a node type.
///
/// This is registered via `inventory` and can be queried at runtime
/// without any async locks.
#[derive(Debug, Clone, Copy)]
pub struct NodeTypeInfo {
    pub name: &'static str,
    pub category: NodeCategory,
    pub inputs: &'static [SlotDef],
    pub outputs: &'static [SlotDef],
    pub default_value: DefaultValue,
}

inventory::collect!(NodeTypeInfo);

/// Get node type info by name.
pub fn get_node_type_info(name: &str) -> Option<&'static NodeTypeInfo> {
    inventory::iter::<NodeTypeInfo>().find(|info| info.name == name)
}

/// Get all registered node types.
pub fn all_node_types() -> impl Iterator<Item = &'static NodeTypeInfo> {
    inventory::iter::<NodeTypeInfo>()
}

/// Get node types grouped by category.
pub fn node_types_by_category() -> HashMap<NodeCategory, Vec<&'static NodeTypeInfo>> {
    let mut map: HashMap<NodeCategory, Vec<&'static NodeTypeInfo>> = HashMap::new();
    for info in inventory::iter::<NodeTypeInfo>() {
        map.entry(info.category).or_default().push(info);
    }
    map
}
