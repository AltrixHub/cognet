//! Add operator node.
//!
//! Adds two numbers (A + B). Every operand carries a scalar port and a `List(Number)`
//! alternative; see [`super::broadcast`] for the elementwise semantics.

use crate::{
    node::operators::broadcast, register_nodes, ExecutionContext, NodeCategory, NodeImpl, NodeMeta,
    SlotDef,
};

/// Adds two numbers (A + B)
#[derive(Debug)]
pub struct AddNode;

impl NodeMeta for AddNode {
    const NAME: &'static str = "Add";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[
        broadcast::number_operand("A"),
        broadcast::number_operand("B"),
        broadcast::list_operand("A[]"),
        broadcast::list_operand("B[]"),
    ];
    const OUTPUTS: &'static [SlotDef] = &[
        broadcast::number_result("Sum"),
        broadcast::list_result("Sums"),
    ];
}

impl NodeImpl for AddNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        broadcast::apply(&ctx, Self::NAME, |a, b| a + b)
    }
}

register_nodes!(AddNode);
