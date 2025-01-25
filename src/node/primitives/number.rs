use crate::{
    impl_node_core, impl_primitive_node_core, Data, DataType, EvaluationContext, InputSlot,
    NodeCore, NodeImpl, OutputSlot,
};

#[derive(Debug)]
pub struct NumberNode {
    node_name: &'static str,
    inputs: Vec<InputSlot>,
    outputs: Vec<OutputSlot>,
}

impl_primitive_node_core!(NumberNode);

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
