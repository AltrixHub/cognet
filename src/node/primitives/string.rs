use crate::{
    register_nodes, DataType, DefaultValue, ExecutionContext, NodeCategory, NodeImpl, NodeMeta,
    SlotDef,
};

#[derive(Debug)]
pub struct StringNode;

impl NodeMeta for StringNode {
    const NAME: &'static str = "String";
    const CATEGORY: NodeCategory = NodeCategory::Primitive;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::String,
        max_connections: None,
        inspector_visible: true,
    }];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::String("Hello");
}

impl NodeImpl for StringNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        let node_data = ctx.node_data.ok_or("Failed to get node data")?;
        ctx.output_writer.set(0, node_data)?;
        Ok(())
    }
}

register_nodes!(StringNode);
