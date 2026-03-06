use crate::{
    register_nodes, Data, DataType, DefaultValue, ExecutionContext, InputSlot, NodeCategory,
    NodeImpl, NodeMeta, OutputSlot, SlotDef,
};

#[derive(Debug)]
pub struct StringNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for StringNode {
    const NAME: &'static str = "String";
    const CATEGORY: NodeCategory = NodeCategory::Primitive;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::String,
        max_connections: None,
    }];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::String("Hello");
}

#[async_trait::async_trait]
impl NodeImpl for StringNode {
    async fn execute(&self, ctx: ExecutionContext) -> Result<(), String> {
        let node_data = ctx.node_data.ok_or("Failed to get node data")?;
        ctx.output_writer.set(0, node_data)?;
        Ok(())
    }
}

register_nodes!(StringNode);
