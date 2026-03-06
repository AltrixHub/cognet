use crate::{
    register_nodes, Data, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

/// Adds two numbers (A + B)
#[derive(Debug)]
pub struct AddNode;

impl NodeMeta for AddNode {
    const NAME: &'static str = "Add";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[
        SlotDef {
            label: "A",
            data_type: DataType::Number,
            max_connections: Some(1),
        },
        SlotDef {
            label: "B",
            data_type: DataType::Number,
            max_connections: Some(1),
        },
    ];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Sum",
        data_type: DataType::Number,
        max_connections: None,
    }];
}

#[async_trait::async_trait]
impl NodeImpl for AddNode {
    async fn execute(&self, ctx: ExecutionContext) -> Result<(), String> {
        let a: f64 = ctx.input_values.first()
            .and_then(|v| v.first())
            .and_then(|d| d.value::<f64>().ok().copied())
            .unwrap_or(0.0);

        let b: f64 = ctx.input_values.get(1)
            .and_then(|v| v.first())
            .and_then(|d| d.value::<f64>().ok().copied())
            .unwrap_or(0.0);

        ctx.output_writer.set(0, Data::new(a + b)?)?;
        Ok(())
    }
}

register_nodes!(AddNode);
