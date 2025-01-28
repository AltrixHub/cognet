use crate::{DataType, InputSlot, NodeImpl, OutputSlot, SharedExecutionCache};

#[derive(Debug)]
pub struct NumberNode {
    pub node_name: &'static str,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

#[async_trait::async_trait]
impl NodeImpl for NumberNode {
    fn initialize() -> Self {
        Self {
            node_name: "Number",
            inputs: vec![],
            outputs: vec![OutputSlot {
                label: "Value",
                data_type: DataType::Number,
                ..Default::default()
            }],
        }
    }

    async fn execute(&self, _cache: SharedExecutionCache) -> Result<(), String> {
        Ok(())
    }
}
