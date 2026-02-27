//! SubGraph input proxy node.
//!
//! Placed inside a subgraph to receive data from the parent graph's
//! SubGraphNode input slots. Each output corresponds to one external input.

#[allow(unused_imports)]
use crate::{
    Data, DefaultValue, InputSlot, NodeCategory, NodeCore, NodeImpl, NodeMeta, NodeValueSetter,
    OutputSlot, SharedExecutionCache, SlotDef,
};

/// Proxy node inside a subgraph that bridges external inputs.
///
/// During subgraph execution, external input data is injected into this node's
/// outputs before the internal graph executes. Each output slot corresponds
/// to one input slot on the parent SubGraphNode.
#[derive(Debug)]
pub struct SubGraphInputNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
}

impl NodeMeta for SubGraphInputNode {
    const NAME: &'static str = "SubGraphInput";
    const CATEGORY: NodeCategory = NodeCategory::Utility;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::None;
}

#[async_trait::async_trait]
impl NodeImpl for SubGraphInputNode {
    async fn execute(&self, _cache: SharedExecutionCache) -> Result<(), String> {
        // Data is injected directly into outputs by SubGraphNode before execution.
        // This node's execute is a no-op.
        Ok(())
    }
}

crate::register_nodes!(SubGraphInputNode);
