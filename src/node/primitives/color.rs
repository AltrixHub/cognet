//! Color primitive node.
//!
//! Outputs an RGBA color value. The node's data holds a `ColorValue`
//! which is passed through to the output.

use crate::{
    register_nodes, Data, DataType, DefaultValue, InputSlot, NodeCategory, NodeCore, NodeImpl,
    NodeMeta, NodeValueSetter, OutputSlot, SharedExecutionCache, SlotDef,
};

/// A Color value node that outputs an RGBA color.
#[derive(Debug)]
pub struct ColorNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for ColorNode {
    const NAME: &'static str = "Color";
    const CATEGORY: NodeCategory = NodeCategory::Primitive;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::Color,
        max_connections: None,
    }];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::Color {
        r: 255.0,
        g: 255.0,
        b: 255.0,
        a: 255.0,
    };
}

#[async_trait::async_trait]
impl NodeImpl for ColorNode {
    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let node_data = self.node_data().ok_or("Failed to get node data")?;
        self.set_output_data(cache, 0, node_data)?;
        Ok(())
    }
}

register_nodes!(ColorNode);
