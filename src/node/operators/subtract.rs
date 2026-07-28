//! Subtract operator node.
//!
//! Subtracts B from A (A - B). Every operand carries a scalar port and a `List(Number)`
//! alternative; see [`super::broadcast`] for the elementwise semantics.

use crate::{
    node::operators::broadcast, register_nodes, ExecutionContext, NodeCategory, NodeImpl, NodeMeta,
    SlotDef,
};

/// Subtracts B from A (A - B)
#[derive(Debug)]
pub struct SubtractNode;

impl NodeMeta for SubtractNode {
    const NAME: &'static str = "Subtract";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[
        broadcast::number_operand("A"),
        broadcast::number_operand("B"),
        broadcast::list_operand("A[]"),
        broadcast::list_operand("B[]"),
    ];
    const OUTPUTS: &'static [SlotDef] = &[
        broadcast::number_result("Difference"),
        broadcast::list_result("Differences"),
    ];
}

impl NodeImpl for SubtractNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        broadcast::apply(&ctx, Self::NAME, |a, b| a - b)
    }
}

register_nodes!(SubtractNode);
