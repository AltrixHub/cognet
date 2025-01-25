use crate::{
    impl_node_core, Data, DataType, EvaluationContext, InputSlot, NodeCore, NodeImpl, OutputSlot,
};

#[derive(Debug)]
pub struct AddListNode {
    node_name: &'static str,
    inputs: Vec<InputSlot>,
    outputs: Vec<OutputSlot>,
}

impl_node_core!(AddListNode);

impl NodeImpl for AddListNode {
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

    fn execute(&self, evaluation_context: &mut EvaluationContext) -> Result<(), String> {
        let data_list = self.input_value(evaluation_context, 0)?;
        let mut result = 0.;
        for data in data_list {
            match data {
                Data::Number(value) => result += value,
                _ => return Err("Expected number".to_string()),
            }
        }
        self.set_output_value(evaluation_context, 0, Data::Number(result))?;
        Ok(())
    }
}
