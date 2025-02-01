use crate::{
    register_nodes, Data, DataType, InputSlot, NodeCore, NodeImpl, NodeValueSetter, OutputSlot,
    SharedExecutionCache,
};

#[derive(Debug)]
pub struct NumberNode {
    pub node_name: &'static str,
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

#[async_trait::async_trait]
impl NodeImpl for NumberNode {
    fn initialize() -> Result<Self, String> {
        Ok(Self {
            node_name: "Number",
            node_data: Some(Data::new(10.)?),
            inputs: vec![],
            outputs: vec![OutputSlot {
                label: "Number",
                data_type: DataType::Number,
                ..Default::default()
            }],
        })
    }

    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let node_data = self.node_data().ok_or(format!("Failed to get node data"))?;
        self.set_output_data(cache, 0, node_data)?;
        Ok(())
    }
}

register_nodes!(NumberNode);
