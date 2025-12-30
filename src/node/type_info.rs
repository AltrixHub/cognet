//! Static metadata for node types.
//!
//! This module provides compile-time metadata about node types,
//! allowing UI to access node information without async locks.

use crate::{Data, DataType, InputSlot, OutputSlot};
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
}

impl DefaultValue {
    /// Convert to Option<Data> at runtime.
    pub fn to_data(self) -> Option<Data> {
        match self {
            DefaultValue::None => None,
            DefaultValue::Number(n) => Data::new(n).ok(),
            DefaultValue::String(s) => Data::new(s.to_string()).ok(),
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

impl SlotDef {
    /// Create an InputSlot from this definition.
    pub fn to_input_slot(&self) -> InputSlot {
        InputSlot {
            label: self.label,
            data_type: self.data_type,
            max_connections: self.max_connections,
            ..Default::default()
        }
    }

    /// Create an OutputSlot from this definition.
    pub fn to_output_slot(&self) -> OutputSlot {
        OutputSlot {
            label: self.label,
            data_type: self.data_type,
            ..Default::default()
        }
    }
}

/// Create InputSlots from a slice of SlotDefs.
pub fn inputs_from_defs(defs: &[SlotDef]) -> Vec<InputSlot> {
    defs.iter().map(|d| d.to_input_slot()).collect()
}

/// Create OutputSlots from a slice of SlotDefs.
pub fn outputs_from_defs(defs: &[SlotDef]) -> Vec<OutputSlot> {
    defs.iter().map(|d| d.to_output_slot()).collect()
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
