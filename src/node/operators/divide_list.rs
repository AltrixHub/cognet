//! Divide list operator node.
//!
//! Divides the first number by all subsequent numbers.
//! Returns 0.0 when any divisor is zero.

use crate::{
    register_nodes, Data, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

/// Divides the first number by all subsequent numbers
#[derive(Debug)]
pub struct DivideListNode;

impl NodeMeta for DivideListNode {
    const NAME: &'static str = "DivideList";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Numbers",
        data_type: DataType::Number,
        max_connections: None,
        inspector_visible: true,
    }];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Quotient",
        data_type: DataType::Number,
        max_connections: None,
        inspector_visible: true,
    }];
}

impl NodeImpl for DivideListNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        let data_list = ctx.input_values.first().cloned().unwrap_or_default();
        let mut result: Option<f64> = None;
        for data in &data_list {
            let value: f64 = *data.value()?;
            match result {
                None => result = Some(value),
                Some(r) => {
                    result = Some(if value == 0.0 { 0.0 } else { r / value });
                }
            }
        }
        ctx.output_writer
            .set(0, Data::new(result.unwrap_or(0.0))?)?;
        Ok(())
    }
}

register_nodes!(DivideListNode);
