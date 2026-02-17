//! Divide operator node.
//!
//! Divides A by B. Returns 0.0 when B is zero.

use crate::{
    register_nodes, Data, DataType, InputSlot, NodeCategory, NodeCore, NodeImpl, NodeMeta,
    NodeValueSetter, OutputSlot, SharedExecutionCache, SlotDef,
};

/// Divides A by B (A / B)
#[derive(Debug)]
pub struct DivideNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for DivideNode {
    const NAME: &'static str = "Divide";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[
        SlotDef {
            label: "A",
            data_type: DataType::Number,
            max_connections: Some(1),
        },
        SlotDef {
            label: "B",
            data_type: DataType::Number,
            max_connections: Some(1),
        },
    ];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Quotient",
        data_type: DataType::Number,
        max_connections: None,
    }];
}

#[async_trait::async_trait]
impl NodeImpl for DivideNode {
    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let a_values = self.input_value(cache.share(), 0)?;
        let b_values = self.input_value(cache.share(), 1)?;

        let mut a: f64 = 0.0;
        for data in a_values {
            a = *data.value()?;
            break;
        }

        let mut b: f64 = 0.0;
        for data in b_values {
            b = *data.value()?;
            break;
        }

        let result = if b == 0.0 { 0.0 } else { a / b };
        self.set_output_data(cache, 0, Data::new(result)?)?;
        Ok(())
    }
}

register_nodes!(DivideNode);
