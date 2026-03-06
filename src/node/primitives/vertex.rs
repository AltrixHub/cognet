//! Vertex primitive node.
//!
//! Outputs a 3D vertex value. The node's data holds a `Vector3`
//! which is passed through to the output. Semantically distinct from
//! Vector3Node — Vertex represents a point/position in space.

use crate::{
    register_nodes, DataType, DefaultValue, ExecutionContext, NodeCategory, NodeImpl, NodeMeta,
    SlotDef,
};

/// A Vertex value node that outputs a 3D position (x, y, z).
#[derive(Debug)]
pub struct VertexNode;

impl NodeMeta for VertexNode {
    const NAME: &'static str = "Vertex";
    const CATEGORY: NodeCategory = NodeCategory::Primitive;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[SlotDef {
        label: "Value",
        data_type: DataType::Vector3,
        max_connections: None,
    }];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::Vector3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
}

#[async_trait::async_trait]
impl NodeImpl for VertexNode {
    async fn execute(&self, ctx: ExecutionContext) -> Result<(), String> {
        let node_data = ctx.node_data.ok_or("Failed to get node data")?;
        ctx.output_writer.set(0, node_data)?;
        Ok(())
    }
}

register_nodes!(VertexNode);
