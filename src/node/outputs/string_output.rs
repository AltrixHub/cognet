use crate::{
    register_nodes, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

#[derive(Debug)]
pub struct StringOutput;

impl NodeMeta for StringOutput {
    const NAME: &'static str = "String Output";
    const CATEGORY: NodeCategory = NodeCategory::Output;
    const INPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::String,
        max_connections: Some(1),
    }];
    const OUTPUTS: &'static [SlotDef] = &[];
}

#[async_trait::async_trait]
impl NodeImpl for StringOutput {
    async fn execute(&self, _ctx: ExecutionContext) -> Result<(), String> {
        // Output node just reads its input - the value is read from cache for UI display
        Ok(())
    }
}

register_nodes!(StringOutput);
