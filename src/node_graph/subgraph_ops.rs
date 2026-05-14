//! SubGraph operations: group_nodes and ungroup_node.
//!
//! plan-006 P3c.10: Both operations have been removed pending the full
//! transparent-SubGraph path-aware rewrite (P3c.11+). The old implementations
//! relied on `SubGraphNode::internal_graph` which is deleted in this phase.
//!
//! `GroupResult` and `UngroupResult` are kept as public types so call sites
//! in the modeling example compile; the operations themselves return errors
//! indicating they are not yet implemented under the new architecture.

use std::collections::HashSet;

use crate::{NodeGraph, NodeId};

/// Result of grouping nodes into a subgraph.
#[derive(Debug)]
pub struct GroupResult {
    /// The NodeId of the new SubGraphNode in the parent graph.
    pub subgraph_node_id: NodeId,
    /// Mapping from original node IDs to internal node IDs inside the subgraph.
    pub node_id_remap: std::collections::HashMap<NodeId, NodeId>,
}

/// Result of ungrouping a subgraph node.
#[derive(Debug)]
pub struct UngroupResult {
    /// NodeIds of nodes that were extracted from the subgraph back into the parent.
    pub extracted_node_ids: Vec<NodeId>,
}

impl NodeGraph {
    /// Group a set of nodes into a SubGraphNode.
    ///
    /// **Not yet implemented in P3c.10.** The transparent-SubGraph group
    /// operation requires path-aware node insertion (P3c.11+).
    pub fn group_nodes(
        &mut self,
        _node_ids: &HashSet<NodeId>,
        _label: impl Into<String>,
    ) -> Result<GroupResult, String> {
        Err(
            "group_nodes is not yet implemented under the transparent-SubGraph \
             architecture (plan-006 P3c.11)"
                .to_string(),
        )
    }

    /// Ungroup a SubGraphNode, extracting its internal nodes back into the parent.
    ///
    /// **Not yet implemented in P3c.10.** The transparent-SubGraph ungroup
    /// operation requires path-aware node extraction (P3c.11+).
    pub fn ungroup_node(&mut self, _subgraph_node_id: NodeId) -> Result<UngroupResult, String> {
        Err(
            "ungroup_node is not yet implemented under the transparent-SubGraph \
             architecture (plan-006 P3c.11)"
                .to_string(),
        )
    }
}
