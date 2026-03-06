use crate::{
    register_nodes, Data, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

/// Subtracts all subsequent numbers from the first number
#[derive(Debug)]
pub struct SubtractListNode;

impl NodeMeta for SubtractListNode {
    const NAME: &'static str = "SubtractList";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Numbers",
        data_type: DataType::Number,
        max_connections: None,
    }];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Difference",
        data_type: DataType::Number,
        max_connections: None,
    }];
}

#[async_trait::async_trait]
impl NodeImpl for SubtractListNode {
    async fn execute(&self, ctx: ExecutionContext) -> Result<(), String> {
        let data_list = ctx.input_values.first().cloned().unwrap_or_default();
        let mut result: Option<f64> = None;
        for data in &data_list {
            let value: f64 = *data.value()?;
            match result {
                None => result = Some(value),
                Some(r) => result = Some(r - value),
            }
        }
        ctx.output_writer.set(0, Data::new(result.unwrap_or(0.0))?)?;
        Ok(())
    }
}

register_nodes!(SubtractListNode);
