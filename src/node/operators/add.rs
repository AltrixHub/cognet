use crate::{
    DataType, InputSlot, NodeCore, NodeImpl, NodeValueSetter, OutputSlot, SharedExecutionCache,
};

#[derive(Debug)]
pub struct AddNode {
    pub node_name: &'static str,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

#[async_trait::async_trait]
impl NodeImpl for AddNode {
    fn initialize() -> Self {
        Self {
            node_name: "Addition",
            inputs: vec![InputSlot {
                label: "Number List",
                data_type: DataType::Number,
                ..Default::default()
            }],
            outputs: vec![OutputSlot {
                label: "Result",
                data_type: DataType::Number,
                ..Default::default()
            }],
        }
    }

    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let data_list = self.input_value(cache.share(), 0)?;
        let mut result = 0.;
        for data in data_list {
            result += data.value()?;
        }
        self.set_output_value(cache, 0, Box::new(result))?;
        Ok(())
    }
}
