//! Divide operator node.
//!
//! Divides A by B. Returns 0.0 when B is zero — the same substitution
//! applies per element in list mode, so the scalar and per-element forms of
//! one node never disagree. Every operand carries a scalar port and a
//! `List(Number)` alternative; see [`super::broadcast`] for the elementwise
//! semantics.

use crate::{
    node::operators::broadcast, register_nodes, ExecutionContext, NodeCategory, NodeImpl, NodeMeta,
    SlotDef,
};

/// Divides A by B (A / B)
#[derive(Debug)]
pub struct DivideNode;

impl NodeMeta for DivideNode {
    const NAME: &'static str = "Divide";
    const CATEGORY: NodeCategory = NodeCategory::Math;
    const INPUTS: &'static [SlotDef] = &[
        broadcast::number_operand("A"),
        broadcast::number_operand("B"),
        broadcast::list_operand("A[]"),
        broadcast::list_operand("B[]"),
    ];
    const OUTPUTS: &'static [SlotDef] = &[
        broadcast::number_result("Quotient"),
        broadcast::list_result("Quotients"),
    ];
}

impl NodeImpl for DivideNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        broadcast::apply(&ctx, Self::NAME, |a, b| if b == 0.0 { 0.0 } else { a / b })
    }
}

register_nodes!(DivideNode);
