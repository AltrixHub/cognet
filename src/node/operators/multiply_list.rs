use crate::{
    register_nodes, Data, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

/// Multiplies all input numbers together
#[derive(Debug)]
pub struct MultiplyListNode;

impl NodeMeta for MultiplyListNode {
    const NAME: &'static str = "MultiplyList";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Numbers",
        data_type: DataType::Number,
        max_connections: None,
        inspector_visible: true,
    }];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Product",
        data_type: DataType::Number,
        max_connections: None,
        inspector_visible: true,
    }];
}

impl NodeImpl for MultiplyListNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        let data_list = ctx.input_values.first().cloned().unwrap_or_default();
        let mut result = 1.0_f64;
        for data in &data_list {
            result *= data.value::<f64>()?;
        }
        ctx.output_writer.set(0, Data::new(result)?)?;
        Ok(())
    }
}

register_nodes!(MultiplyListNode);
