//! SubGraph convenience methods for NodeGraph.
//!
//! These methods provide direct access to SubGraphNode operations
//! without requiring callers to perform entity lookup + downcast manually.

use crate::{Data, DataType, InputSlot, NodeGraph, NodeGraphAPI, NodeId, OutputSlot, SubGraphNode};

use super::EdgeInfo;

impl NodeGraph {
    // === SubGraph detection & info ===

    /// Check if a node is a SubGraphNode.
    pub fn is_subgraph_node(&self, node_id: &NodeId) -> bool {
        self.get_node_by_id(node_id)
            .and_then(|entity| {
                entity
                    .read()
                    .ok()
                    .map(|guard| guard.as_any().downcast_ref::<SubGraphNode>().is_some())
            })
            .unwrap_or(false)
    }

    /// Get the display label of a SubGraphNode.
    pub fn subgraph_label(&self, node_id: &NodeId) -> Option<String> {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some(sg.label().to_string())
    }

    /// Set the display label of a SubGraphNode.
    ///
    /// Returns true if successful.
    pub fn set_subgraph_label(&self, node_id: &NodeId, label: &str) -> bool {
        let entity = match self.get_node_by_id(node_id) {
            Some(e) => e,
            None => return false,
        };
        let mut guard = match entity.write() {
            Ok(g) => g,
            Err(_) => return false,
        };
        match guard.as_any_mut().downcast_mut::<SubGraphNode>() {
            Some(sg) => {
                sg.set_label(label);
                true
            }
            None => false,
        }
    }

    /// Get the proxy node IDs for a SubGraphNode.
    ///
    /// Returns `(input_proxy_id, output_proxy_id)`.
    pub fn subgraph_proxy_ids(&self, node_id: &NodeId) -> Option<(NodeId, NodeId)> {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some((sg.input_proxy_id(), sg.output_proxy_id()))
    }

    // === SubGraph internal access ===

    /// Get all node IDs in a SubGraphNode's internal graph.
    pub fn subgraph_node_ids(&self, node_id: &NodeId) -> Option<Vec<NodeId>> {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        let ns = sg.internal_graph().node_states().read().ok()?;
        Some(ns.node_ids().copied().collect())
    }

    /// Get all edges in a SubGraphNode's internal graph.
    pub fn subgraph_edges(&self, node_id: &NodeId) -> Vec<EdgeInfo> {
        let info = (|| -> Option<Vec<EdgeInfo>> {
            let entity = self.get_node_by_id(node_id)?;
            let guard = entity.read().ok()?;
            let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
            let cache = sg.internal_graph().shared_cache();
            let cache_read = cache.read().ok()?;
            Some(
                cache_read
                    .edges
                    .iter()
                    .map(|(id, edge)| EdgeInfo {
                        id: *id,
                        from_node: edge.from_node_id,
                        from_output: edge.from_output_slot_index,
                        to_node: edge.to_node_id,
                        to_input: edge.to_input_slot_index,
                    })
                    .collect(),
            )
        })();
        info.unwrap_or_default()
    }

    /// Execute a closure with read access to a SubGraphNode's internal graph.
    pub fn with_subgraph<F, R>(&self, node_id: &NodeId, f: F) -> Option<R>
    where
        F: FnOnce(&NodeGraph) -> R,
    {
        let entity = self.get_node_by_id(node_id)?;
        let guard = entity.read().ok()?;
        let sg = guard.as_any().downcast_ref::<SubGraphNode>()?;
        Some(f(sg.internal_graph()))
    }

    /// Execute a closure with mutable access to a SubGraphNode's internal graph.
    ///
    /// Note: Takes `&self` because mutation happens inside the SubGraphNode entity
    /// (which has its own RwLock), not on the outer NodeGraph.
    pub fn with_subgraph_mut<F, R>(&self, node_id: &NodeId, f: F) -> Option<R>
    where
        F: FnOnce(&mut NodeGraph) -> R,
    {
        let entity = self.get_node_by_id(node_id)?;
        let mut guard = entity.write().ok()?;
        let sg = guard.as_any_mut().downcast_mut::<SubGraphNode>()?;
        Some(f(sg.internal_graph_mut()))
    }

    // === SubGraph port management ===

    /// Add an input port to a SubGraphNode.
    pub fn add_subgraph_input(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        let slot = InputSlot {
            label,
            data_type,
            ..Default::default()
        };
        sg.add_input(slot)
    }

    /// Remove an input port from a SubGraphNode by index.
    pub fn remove_subgraph_input(
        &self,
        node_id: &NodeId,
        index: usize,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.remove_input(index)
    }

    /// Add an output port to a SubGraphNode.
    pub fn add_subgraph_output(
        &self,
        node_id: &NodeId,
        label: &'static str,
        data_type: DataType,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        let slot = OutputSlot {
            label,
            data_type,
            ..Default::default()
        };
        sg.add_output(slot)
    }

    /// Remove an output port from a SubGraphNode by index.
    pub fn remove_subgraph_output(
        &self,
        node_id: &NodeId,
        index: usize,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.remove_output(index)
    }

    /// Set the default value for a SubGraphNode input port.
    pub fn set_subgraph_input_default(
        &self,
        node_id: &NodeId,
        input_index: usize,
        data: Data,
    ) -> Result<(), String> {
        let entity = self
            .get_node_by_id(node_id)
            .ok_or_else(|| format!("Node {:?} not found", node_id))?;
        let mut guard = entity.write().map_err(|e| e.to_string())?;
        let sg = guard
            .as_any_mut()
            .downcast_mut::<SubGraphNode>()
            .ok_or("Node is not a SubGraphNode")?;
        sg.set_input_default(input_index, data)
    }
}

#[cfg(test)]
mod tests {
    use crate::NodeGraphAPI;

    #[tokio::test]
    async fn test_is_subgraph_node() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let num = graph
            .create_node::<crate::NumberNode>()
            .expect("create num");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        assert!(!graph.is_subgraph_node(&num));
        assert!(graph.is_subgraph_node(&sg));
    }

    #[tokio::test]
    async fn test_subgraph_label() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        assert_eq!(graph.subgraph_label(&sg), Some("SubGraph".to_string()));
        assert!(graph.set_subgraph_label(&sg, "Custom"));
        assert_eq!(graph.subgraph_label(&sg), Some("Custom".to_string()));
    }

    #[tokio::test]
    async fn test_subgraph_proxy_ids() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        let (input_id, output_id) = graph.subgraph_proxy_ids(&sg).expect("proxy ids");
        // Proxy IDs should be valid NodeIds in the internal graph
        let internal_ids = graph.subgraph_node_ids(&sg).expect("internal ids");
        assert!(internal_ids.contains(&input_id));
        assert!(internal_ids.contains(&output_id));
    }

    #[tokio::test]
    async fn test_add_remove_subgraph_ports() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Add input
        graph
            .add_subgraph_input(&sg, "In1", crate::DataType::Number)
            .expect("add input");

        // Add output
        graph
            .add_subgraph_output(&sg, "Out1", crate::DataType::Number)
            .expect("add output");

        // Verify via entity
        let entity = graph.get_node_by_id(&sg).unwrap();
        let guard = entity.read().unwrap();
        assert_eq!(guard.inputs().len(), 1);
        assert_eq!(guard.outputs().len(), 1);
        drop(guard);

        // Remove
        graph.remove_subgraph_input(&sg, 0).expect("remove input");
        graph
            .remove_subgraph_output(&sg, 0)
            .expect("remove output");

        let guard = entity.read().unwrap();
        assert!(guard.inputs().is_empty());
        assert!(guard.outputs().is_empty());
    }

    #[tokio::test]
    async fn test_with_subgraph() {
        let mut graph = crate::NodeGraph::new().expect("create graph");
        let sg = graph
            .create_node::<crate::SubGraphNode>()
            .expect("create subgraph");

        // Read access
        let count = graph
            .with_subgraph(&sg, |internal| internal.node_ids().len())
            .expect("with_subgraph");
        assert_eq!(count, 2); // input + output proxies

        // Mut access: create a node inside
        let internal_id = graph
            .with_subgraph_mut(&sg, |internal| {
                internal.create_node_by_name("Number")
            })
            .expect("with_subgraph_mut")
            .expect("create internal node");

        let ids = graph.subgraph_node_ids(&sg).expect("subgraph node ids");
        assert!(ids.contains(&internal_id));
    }
}
