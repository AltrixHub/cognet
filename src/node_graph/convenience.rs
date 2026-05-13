//! Convenience methods for NodeGraph.
//!
//! These methods provide simpler access to common graph queries
//! without requiring callers to navigate the storage layer directly.

use crate::{EdgeId, NodeGraph, NodeId};

use super::EdgeInfo;

impl NodeGraph {
    /// Get all node IDs in the graph.
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.node_states
            .read()
            .map(|guard| guard.node_ids().collect())
            .unwrap_or_default()
    }

    /// Get all edges as `EdgeInfo` structs.
    ///
    /// Reads from NodeStates (sole owner of edge data).
    pub fn edges_info(&self) -> Vec<EdgeInfo> {
        self.node_states
            .read()
            .map(|guard| {
                guard
                    .edges()
                    .iter()
                    .map(|(id, edge)| EdgeInfo {
                        id: *id,
                        from_node: edge.from_node_id,
                        from_output: edge.from_output_slot_index,
                        to_node: edge.to_node_id,
                        to_input: edge.to_input_slot_index,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get a specific edge by ID as `EdgeInfo`.
    pub fn edge_info(&self, edge_id: &EdgeId) -> Option<EdgeInfo> {
        self.node_states.read().ok().and_then(|guard| {
            guard.get_edge(edge_id).map(|edge| EdgeInfo {
                id: *edge_id,
                from_node: edge.from_node_id,
                from_output: edge.from_output_slot_index,
                to_node: edge.to_node_id,
                to_input: edge.to_input_slot_index,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::NodeGraphWrite;

    #[test]
    fn test_node_ids() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        assert!(graph.node_ids().is_empty());

        let id1 = graph
            .create_node::<crate::NumberNode>()
            .expect("create node");
        let id2 = graph
            .create_node::<crate::NumberNode>()
            .expect("create node");

        let ids = graph.node_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_edges_info() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let num = graph
            .create_node::<crate::NumberNode>()
            .expect("create num");
        let add = graph.create_node::<crate::AddNode>().expect("create add");

        let edge_id = graph
            .connect_nodes(&num, 0, &add, 0)
            .expect("connect nodes");

        let edges = graph.edges_info();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].id, edge_id);
        assert_eq!(edges[0].from_node, num);
        assert_eq!(edges[0].from_output, 0);
        assert_eq!(edges[0].to_node, add);
        assert_eq!(edges[0].to_input, 0);
    }

    #[test]
    fn test_edge_info() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let num = graph
            .create_node::<crate::NumberNode>()
            .expect("create num");
        let add = graph.create_node::<crate::AddNode>().expect("create add");

        let edge_id = graph
            .connect_nodes(&num, 0, &add, 0)
            .expect("connect nodes");

        let info = graph.edge_info(&edge_id).expect("edge exists");
        assert_eq!(info.from_node, num);
        assert_eq!(info.to_node, add);

        let fake_id = crate::EdgeId::new();
        assert!(graph.edge_info(&fake_id).is_none());
    }
}
