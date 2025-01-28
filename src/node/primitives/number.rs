use std::sync::{Arc, Mutex};

use crate::{DataType, EvaluationContext, InputSlot, NodeImpl, OutputSlot};

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

    async fn execute(
        &self,
        _evaluation_context: Arc<Mutex<EvaluationContext>>,
    ) -> Result<(), String> {
        Ok(())
    }
}
