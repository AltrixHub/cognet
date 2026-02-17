//! Vector3 primitive node.
//!
//! Outputs a 3D vector value. The node's data holds a `Vector3`
//! which is passed through to the output.

use crate::{
    register_nodes, Data, DataType, DefaultValue, InputSlot, NodeCategory, NodeCore, NodeImpl,
    NodeMeta, NodeValueSetter, OutputSlot, SharedExecutionCache, SlotDef,
};

/// A Vector3 value node that outputs a 3D vector (x, y, z).
#[derive(Debug)]
pub struct Vector3Node {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for Vector3Node {
    const NAME: &'static str = "Vector3";
    const CATEGORY: NodeCategory = NodeCategory::Primitive;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::Vector3,
        max_connections: None,
    }];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::Vector3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
}

#[async_trait::async_trait]
impl NodeImpl for Vector3Node {
    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let node_data = self.node_data().ok_or("Failed to get node data")?;
        self.set_output_data(cache, 0, node_data)?;
        Ok(())
    }
}

register_nodes!(Vector3Node);
