use crate::{DataType, EvaluationContext, InputSlot, NodeImpl, OutputSlot};

#[derive(Debug)]
pub struct NumberNode {
    pub node_name: &'static str,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

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

    fn execute(&self, _evaluation_context: &mut EvaluationContext) -> Result<(), String> {
        Ok(())
    }
}
