use crate::{
    register_nodes, Data, DataType, InputSlot, NodeCategory, NodeCore, NodeImpl, NodeMeta,
    OutputSlot, SharedExecutionCache, SlotDef,
};

#[derive(Debug)]
pub struct StringOutput {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

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
    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        // Output node just reads its input - the value is read from cache for UI display
        let _values = self.input_value(cache, 0)?;
        Ok(())
    }
}

register_nodes!(StringOutput);
