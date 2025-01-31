use std::sync::Arc;

use crate::{DataType, InputSlot, NodeImpl, NodeValueSetter, OutputSlot, SharedExecutionCache};

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
            inputs: vec![InputSlot {
                label: "Number",
                data_type: DataType::Number,
                default_value: Some(Arc::new(10.)),
                ..Default::default()
            }],
            outputs: vec![OutputSlot {
                label: "Number",
                data_type: DataType::Number,
                ..Default::default()
            }],
        }
    }

    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String> {
        let number_slot = self.inputs.get(0).ok_or("Invalid output slot index")?;
        let output = number_slot.default_value();

        if let Some(value) = output {
            self.set_output_value(cache, 0, Arc::clone(value))?;
            Ok(())
        } else {
            Err("Output value is None".to_string())
        }
    }
}
