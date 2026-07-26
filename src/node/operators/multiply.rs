//! Multiply operator node.
//!
//! Multiplies two numbers (A * B). Every operand carries a scalar port and a `List(Number)`
//! alternative; see [`super::broadcast`] for the elementwise semantics.

use crate::{
    node::operators::broadcast, register_nodes, ExecutionContext, NodeCategory, NodeImpl, NodeMeta,
    SlotDef,
};

/// Multiplies two numbers (A * B)
#[derive(Debug)]
pub struct MultiplyNode;

impl NodeMeta for MultiplyNode {
    const NAME: &'static str = "Multiply";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[
        broadcast::number_operand("A"),
        broadcast::number_operand("B"),
        broadcast::list_operand("A[]"),
        broadcast::list_operand("B[]"),
    ];
    const OUTPUTS: &'static [SlotDef] = &[
        broadcast::number_result("Product"),
        broadcast::list_result("Products"),
    ];
}

impl NodeImpl for MultiplyNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        broadcast::apply(&ctx, Self::NAME, |a, b| a * b)
    }
}

register_nodes!(MultiplyNode);
