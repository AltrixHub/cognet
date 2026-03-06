use crate::{
    register_nodes, Data, DataType, ExecutionContext, InputSlot, NodeCategory, NodeImpl, NodeMeta,
    OutputSlot, SlotDef,
};

#[derive(Debug)]
pub struct MultiplyNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for MultiplyNode {
    const NAME: &'static str = "Multiply";
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
        label: "Product",
        data_type: DataType::Number,
        max_connections: None,
    }];
}

#[async_trait::async_trait]
impl NodeImpl for MultiplyNode {
    async fn execute(&self, ctx: ExecutionContext) -> Result<(), String> {
        let a: f64 = ctx.input_values.get(0)
            .and_then(|v| v.first())
            .and_then(|d| d.value::<f64>().ok().copied())
            .unwrap_or(0.0);

        let b: f64 = ctx.input_values.get(1)
            .and_then(|v| v.first())
            .and_then(|d| d.value::<f64>().ok().copied())
            .unwrap_or(0.0);

        ctx.output_writer.set(0, Data::new(a * b)?)?;
        Ok(())
    }
}

register_nodes!(MultiplyNode);
