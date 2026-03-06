//! Divide operator node.
//!
//! Divides A by B. Returns 0.0 when B is zero.

use crate::{
    register_nodes, Data, DataType, ExecutionContext, InputSlot, NodeCategory, NodeImpl, NodeMeta,
    OutputSlot, SlotDef,
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
    async fn execute(&self, ctx: ExecutionContext) -> Result<(), String> {
        let a: f64 = ctx.input_values.get(0)
            .and_then(|v| v.first())
            .and_then(|d| d.value::<f64>().ok().copied())
            .unwrap_or(0.0);

        let b: f64 = ctx.input_values.get(1)
            .and_then(|v| v.first())
            .and_then(|d| d.value::<f64>().ok().copied())
            .unwrap_or(0.0);

        let result = if b == 0.0 { 0.0 } else { a / b };
        ctx.output_writer.set(0, Data::new(result)?)?;
        Ok(())
    }
}

register_nodes!(DivideNode);
