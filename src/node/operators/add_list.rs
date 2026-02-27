use crate::{
    register_nodes, Data, DataType, InputSlot, NodeCategory, NodeCore, NodeImpl, NodeMeta,
    NodeValueSetter, OutputSlot, SharedExecutionCache, SlotDef,
};

/// Adds all input numbers together
#[derive(Debug)]
pub struct AddListNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

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

#[async_trait::async_trait]
impl NodeImpl for AddListNode {
    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let data_list = self.input_value(cache.share(), 0)?;
        let mut result = 0.;
        for data in data_list {
            result += data.value()?;
        }
        self.set_output_data(cache, 0, Data::new(result)?)?;
        Ok(())
    }
}

register_nodes!(AddListNode);
