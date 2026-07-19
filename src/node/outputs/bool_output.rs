use crate::{
    register_nodes, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

#[derive(Debug)]
pub struct BoolOutput;

impl NodeMeta for BoolOutput {
    const NAME: &'static str = "Bool Output";
    const CATEGORY: NodeCategory = NodeCategory::Output;
    const INPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::Bool,
        max_connections: Some(1),
        inspector_visible: true,
    }];
    const OUTPUTS: &'static [SlotDef] = &[];
}

impl NodeImpl for BoolOutput {
    fn execute_sync(&self, _ctx: ExecutionContext) -> Result<(), String> {
        // Output node just reads its input - the value is read from cache for UI display
        Ok(())
    }
}

register_nodes!(BoolOutput);
