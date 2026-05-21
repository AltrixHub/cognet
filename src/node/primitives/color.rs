//! Color primitive node.
//!
//! Outputs an RGBA color value. The node's data holds a `ColorValue`
//! which is passed through to the output.

use crate::{
    register_nodes, DataType, DefaultValue, ExecutionContext, NodeCategory, NodeImpl, NodeMeta,
    SlotDef,
};

/// A Color value node that outputs an RGBA color.
#[derive(Debug)]
pub struct ColorNode;

impl NodeMeta for ColorNode {
    const NAME: &'static str = "Color";
    const CATEGORY: NodeCategory = NodeCategory::Primitive;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::Color,
        max_connections: None,
    }];
    // `ColorValue` channels are canonically 0.0..=1.0 — see the doc on
    // `ColorValue` in `edge/slot.rs`. White = fully-opaque
    // `(1.0, 1.0, 1.0, 1.0)`.
    const DEFAULT_VALUE: DefaultValue = DefaultValue::Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
}

impl NodeImpl for ColorNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        let node_data = ctx.node_data.ok_or("Failed to get node data")?;
        ctx.output_writer.set(0, node_data)?;
        Ok(())
    }
}

register_nodes!(ColorNode);
