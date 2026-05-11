//! SubGraph output proxy node.
//!
//! Placed inside a subgraph to collect results that will be forwarded
//! to the parent graph's SubGraphNode output slots.

use crate::{DefaultValue, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef};

/// Proxy node inside a subgraph that bridges outputs to the parent.
///
/// After the internal graph executes, the SubGraphNode reads data from
/// this node's inputs and writes them to its own output slots.
/// Each input slot corresponds to one output slot on the parent SubGraphNode.
///
/// # Mirror outputs
///
/// Each input slot also has a matching OUTPUT slot at the same index
/// (added together by [`SubGraphNode::add_output`]). During execution
/// this proxy copies its resolved input values into the matching mirror
/// output, so internal nodes can subscribe to the values that flow out
/// of the SubGraph without re-reading the source.
#[derive(Debug)]
pub struct SubGraphOutputNode;

impl NodeMeta for SubGraphOutputNode {
    const NAME: &'static str = "SubGraphOutput";
    const CATEGORY: NodeCategory = NodeCategory::Utility;
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[];
    const DEFAULT_VALUE: DefaultValue = DefaultValue::None;
}

impl NodeImpl for SubGraphOutputNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        // For each input slot, copy the first resolved value into the
        // matching mirror output slot at the same index. The mirror lets
        // internal nodes observe the values that the SubGraphNode is
        // about to publish to the parent graph.
        //
        // SubGraphNode::collect_outputs is still the source of truth for
        // the parent's external output slots; the mirror is purely an
        // internal convenience.
        for (slot_index, values) in ctx.input_values.iter().enumerate() {
            if let Some(data) = values.first() {
                // Silently skip slots that have no matching mirror output
                // (e.g. older SubGraphNodes constructed before the mirror
                // extension was added) — set() returns an out-of-range
                // error which we treat as a no-op for the mirror path.
                let _ = ctx.output_writer.set(slot_index, data.share());
            }
        }
        Ok(())
    }
}

crate::register_nodes!(SubGraphOutputNode);
