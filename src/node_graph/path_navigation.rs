//! Path-based navigation for nested SubGraph access.
//!
//! These methods allow accessing arbitrary depths of nested SubGraphNodes
//! using a path of NodeIds. Each NodeId in the path refers to a SubGraphNode
//! whose internal graph is entered.
//!
//! - Empty path = root graph
//! - `[A]` = A's internal graph
//! - `[A, B]` = B's internal graph inside A's internal graph

use crate::{Data, NodeGraph, NodeGraphRead, NodeId, SubGraphNode};

use super::EdgeInfo;

impl NodeGraph {
    /// Read access to the graph at a given subgraph path.
    ///
    /// Empty path returns the result of applying `f` to `self`.
    pub fn with_graph_at_path<F, R>(&self, path: &[NodeId], f: F) -> Option<R>
    where
        F: FnOnce(&NodeGraph) -> R,
    {
        if path.is_empty() {
            return Some(f(self));
        }
        traverse_read(self, path, f)
    }

    /// Mutable access to the graph at a given subgraph path.
    ///
    /// Empty path returns the result of applying `f` to `self` (requires `&mut self`).
    /// For non-empty paths, takes `&self` because mutation happens inside
    /// SubGraphNode entities (which have their own RwLock).
    pub fn with_graph_at_path_mut<F, R>(&self, path: &[NodeId], f: F) -> Option<R>
    where
        F: FnOnce(&mut NodeGraph) -> R,
    {
        if path.is_empty() {
            // Can't mutate self with only &self for empty path.
            // Caller should use &mut self directly for root mutations.
            return None;
        }
        traverse_mutate(self, path, f)
    }

    /// Get all node IDs at a subgraph path.
    pub fn node_ids_at_path(&self, path: &[NodeId]) -> Option<Vec<NodeId>> {
        self.with_graph_at_path(path, |graph| graph.node_ids())
    }

    /// Get all edges at a subgraph path.
    pub fn edges_at_path(&self, path: &[NodeId]) -> Vec<EdgeInfo> {
        self.with_graph_at_path(path, |graph| graph.edges_info())
            .unwrap_or_default()
    }

    /// Get proxy node IDs for a SubGraphNode at a given path.
    ///
    /// The path points to the parent graph level, and `node_id` is a
    /// SubGraphNode within that graph.
    pub fn subgraph_proxy_ids_at_path(
        &self,
        path: &[NodeId],
        node_id: &NodeId,
    ) -> Option<(NodeId, NodeId)> {
        self.with_graph_at_path(path, |graph| graph.subgraph_proxy_ids(node_id))
            .flatten()
    }

    /// Check if a node at a subgraph path is a SubGraphNode.
    pub fn is_subgraph_node_at_path(&self, path: &[NodeId], node_id: &NodeId) -> bool {
        self.with_graph_at_path(path, |graph| graph.is_subgraph_node(node_id))
            .unwrap_or(false)
    }

    /// Set a SubGraphNode's input default value at a subgraph path.
    pub fn set_subgraph_input_default_at_path(
        &self,
        path: &[NodeId],
        node_id: &NodeId,
        input_index: usize,
        data: Data,
    ) -> bool {
        if path.is_empty() {
            return self
                .set_subgraph_input_default(node_id, input_index, data)
                .is_ok();
        }
        self.with_graph_at_path(path, |graph| {
            graph
                .set_subgraph_input_default(node_id, input_index, data)
                .is_ok()
        })
        .unwrap_or(false)
    }
}

/// Recursively traverse SubGraphNode internal graphs for read access.
fn traverse_read<F, R>(graph: &NodeGraph, path: &[NodeId], f: F) -> Option<R>
where
    F: FnOnce(&NodeGraph) -> R,
{
    let entity = graph.get_node_by_id(&path[0])?;
    let guard = entity.read().ok()?;
    let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
    if path.len() == 1 {
        Some(f(sg.internal_graph()))
    } else {
        traverse_read(sg.internal_graph(), &path[1..], f)
    }
}

/// Recursively traverse SubGraphNode internal graphs for mutable access.
fn traverse_mutate<F, R>(graph: &NodeGraph, path: &[NodeId], f: F) -> Option<R>
where
    F: FnOnce(&mut NodeGraph) -> R,
{
    let entity = graph.get_node_by_id(&path[0])?;
    if path.len() == 1 {
        let mut guard = entity.write().ok()?;
        let sg = guard.as_any_mut().downcast_mut::<SubGraphNode>()?;
        Some(f(sg.internal_graph_mut()))
    } else {
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        traverse_mutate(sg.internal_graph(), &path[1..], f)
    }
}

#[cfg(test)]
mod tests {
    use crate::NodeGraphWrite;

    #[test]
    fn test_with_graph_at_path_empty() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let num = graph
            .create_node::<crate::NumberNode>()
            .expect("create num");

        // Empty path = root graph
        let ids = graph
            .with_graph_at_path(&[], |g| g.node_ids())
            .expect("root access");
        assert!(ids.contains(&num));
    }

    #[test]
    fn test_with_graph_at_path_single() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Create a node inside the subgraph
        let internal_id = graph
            .with_subgraph_mut(&sg, |internal| internal.create_node_by_name("Number"))
            .expect("access subgraph")
            .expect("create internal node");

        // Path [sg] = sg's internal graph
        let ids = graph
            .with_graph_at_path(&[sg], |g| g.node_ids())
            .expect("path access");
        assert!(ids.contains(&internal_id));
    }

    #[test]
    fn test_with_graph_at_path_mut() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Empty path returns None (can't mutate self with &self)
        assert!(graph.with_graph_at_path_mut(&[], |_g| ()).is_none());

        // Non-empty path: mutate inside subgraph
        let id = graph
            .with_graph_at_path_mut(&[sg], |g| g.create_node_by_name("Number"))
            .expect("path mut access")
            .expect("create node");

        let ids = graph.node_ids_at_path(&[sg]).expect("node ids at path");
        assert!(ids.contains(&id));
    }

    #[test]
    fn test_node_ids_at_path() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        let ids = graph.node_ids_at_path(&[sg]).expect("ids at path");
        // Should contain at least the two proxy nodes
        assert!(ids.len() >= 2);
    }

    #[test]
    fn test_edges_at_path() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Initially no edges inside subgraph
        let edges = graph.edges_at_path(&[sg]);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_is_subgraph_node_at_path() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Create a nested subgraph inside sg
        let nested_sg = graph
            .with_subgraph_mut(&sg, |internal| {
                internal.create_node::<crate::SubGraphNode>()
            })
            .expect("access subgraph")
            .expect("create nested subgraph");

        assert!(graph.is_subgraph_node_at_path(&[sg], &nested_sg));

        let (input_proxy, _) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        assert!(!graph.is_subgraph_node_at_path(&[sg], &input_proxy));
    }
}
