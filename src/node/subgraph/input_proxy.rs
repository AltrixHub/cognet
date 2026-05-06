//! SubGraph input proxy node.
//!
//! Placed inside a subgraph to receive data from the parent graph's
//! SubGraphNode input slots. Each output corresponds to one external input.

use crate::{DefaultValue, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef};

/// Proxy node inside a subgraph that bridges external inputs.
///
/// During subgraph execution, external input data is injected into this node's
/// outputs before the internal graph executes. Each output slot corresponds
/// to one input slot on the parent SubGraphNode.
#[derive(Debug)]
pub struct SubGraphInputNode;

impl NodeMeta for SubGraphInputNode {
    const NAME: &'static str = "SubGraphInput";
    const CATEGORY: NodeCategory = NodeCategory::Utility;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::None;
}

impl NodeImpl for SubGraphInputNode {
    fn execute_sync(&self, _ctx: ExecutionContext) -> Result<(), String> {
        // Data is injected directly into outputs by SubGraphNode before execution.
        // This node's execute is a no-op.
        Ok(())
    }
}

crate::register_nodes!(SubGraphInputNode);
