//! Divide list operator node.
//!
//! Divides the first number by all subsequent numbers.
//! Returns 0.0 when any divisor is zero.

use crate::{
    register_nodes, Data, DataType, InputSlot, NodeCategory, NodeCore, NodeImpl, NodeMeta,
    NodeValueSetter, OutputSlot, SharedExecutionCache, SlotDef,
};

/// Divides the first number by all subsequent numbers
#[derive(Debug)]
pub struct DivideListNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for DivideListNode {
    const NAME: &'static str = "DivideList";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Numbers",
        data_type: DataType::Number,
        max_connections: None,
    }];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Quotient",
        data_type: DataType::Number,
        max_connections: None,
    }];
}

#[async_trait::async_trait]
impl NodeImpl for DivideListNode {
    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let data_list = self.input_value(cache.share(), 0)?;
        let mut result: Option<f64> = None;
        for data in data_list {
            let value: f64 = *data.value()?;
            match result {
                None => result = Some(value),
                Some(r) => {
                    result = Some(if value == 0.0 { 0.0 } else { r / value });
                }
            }
        }
        self.set_output_data(cache, 0, Data::new(result.unwrap_or(0.0))?)?;
        Ok(())
    }
}

register_nodes!(DivideListNode);
