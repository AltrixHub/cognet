use crate::{
    register_nodes, Data, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

/// Adds all input numbers together
#[derive(Debug)]
pub struct AddListNode;

impl NodeMeta for AddListNode {
    const NAME: &'static str = "AddList";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Numbers",
        data_type: DataType::Number,
        max_connections: None,
    }];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Sum",
        data_type: DataType::Number,
        max_connections: None,
    }];
}

impl NodeImpl for AddListNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        let data_list = ctx.input_values.first().cloned().unwrap_or_default();
        let mut result = 0.0_f64;
        for data in &data_list {
            result += data.value::<f64>()?;
        }
        ctx.output_writer.set(0, Data::new(result)?)?;
        Ok(())
    }
}

register_nodes!(AddListNode);
