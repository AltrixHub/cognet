//! SubGraph output proxy node.
//!
//! Placed inside a subgraph to collect results that will be forwarded
//! to the parent graph's SubGraphNode output slots.

#[allow(unused_imports)]
use crate::{
    Data, DefaultValue, ExecutionContext, InputSlot, NodeCategory, NodeImpl, NodeMeta, OutputSlot,
    SlotDef,
};

/// Proxy node inside a subgraph that bridges outputs to the parent.
///
/// After the internal graph executes, the SubGraphNode reads data from
/// this node's inputs and writes them to its own output slots.
/// Each input slot corresponds to one output slot on the parent SubGraphNode.
#[derive(Debug)]
pub struct SubGraphOutputNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for SubGraphOutputNode {
    const NAME: &'static str = "SubGraphOutput";
    const CATEGORY: NodeCategory = NodeCategory::Utility;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::None;
}

#[async_trait::async_trait]
impl NodeImpl for SubGraphOutputNode {
    async fn execute(&self, _ctx: ExecutionContext) -> Result<(), String> {
        // This node simply holds input data that was written by internal nodes.
        // SubGraphNode reads the data from this node's inputs after execution.
        Ok(())
    }
}

crate::register_nodes!(SubGraphOutputNode);
