use futures::future::join_all;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    node_graph_system::NodeGraphSystem, Data, Edge, EdgeId, NodeEntity, NodeGraph, NodeId,
    NodeImpl, NodeManager, NodePrimitive, SharedData,
};

pub trait NodeGraphAPI {
    fn new() -> Self;

    fn node_manager(&self) -> &NodeManager;

    fn node_manager_mut(&mut self) -> &mut NodeManager;

    async fn execute(&mut self) -> Result<(), String>;

    async fn remove_node(&mut self, node_id: NodeId) -> Result<(), String>;

    async fn update_node<T: 'static + NodeImpl>(
        &mut self,
        node_id: NodeId,
        new_node: T,
    ) -> Result<(), String>;

    async fn get_node_by_id(&self, node_id: &NodeId) -> Option<NodeEntity>;

    async fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId>;

    async fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)>;

    async fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String>;

    async fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), String>;

    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, &'static str>;

    async fn get_output_value(
        &self,
        node_id: &NodeId,
        output_slot_index: usize,
    ) -> Option<SharedData>;

    async fn set_default_value<T: 'static + NodePrimitive + NodeImpl>(
        &mut self,
        node_id: &NodeId,
        value: Data,
    ) -> Result<(), String>;
}

impl NodeGraphAPI for NodeGraph {
    fn new() -> Self {
        Self::default()
    }

    fn node_manager(&self) -> &NodeManager {
        &self.node_manager
    }

    fn node_manager_mut(&mut self) -> &mut NodeManager {
        &mut self.node_manager
    }

    async fn execute(&mut self) -> Result<(), String> {
        if self.dirty_nodes.is_empty() {
            return Ok(());
        }

        let sorted_node_levels = self.topological_sort(&self.dirty_nodes)?;

        let nodes = self.node_manager.nodes();
        let context = Arc::clone(&self.context);

        for level_nodes in sorted_node_levels {
            let tasks = level_nodes
                .into_iter()
                .map(|node_id| {
                    let nodes = Arc::clone(&nodes);
                    let context = Arc::clone(&context);
                    tokio::spawn(async move {
                        let nodes = nodes.lock().await;
                        let node = nodes.get(&node_id).cloned();
                        if let Some(node) = node {
                            let node_write = node.read().await;
                            node_write.execute(context).await?;
                        }
                        Ok::<(), String>(())
                    })
                })
                .collect::<Vec<_>>();

            let results = join_all(tasks).await;

            for result in results {
                result.map_err(|e| e.to_string())??;
            }
        }

        self.dirty_nodes.clear();
        Ok(())
    }

    async fn remove_node(&mut self, node_id: NodeId) -> Result<(), String> {
        if self.node_manager.node_remove(&node_id).await.is_some() {
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id.clone()])?;
            self.remove_edges_from_context(&node_id)?;
            self.mark_dirty_nodes(dirty_nodes);
            Ok(())
        } else {
            Err("Node not found.".to_string())
        }
    }

    async fn update_node<T: 'static + NodeImpl>(
        &mut self,
        node_id: NodeId,
        new_node: T,
    ) -> Result<(), String> {
        if self.node_manager.contains_key(&node_id).await {
            let node_arc = Arc::new(RwLock::new(new_node));
            self.node_manager
                .node_insert(node_id.clone(), node_arc)
                .await;
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id])?;
            self.mark_dirty_nodes(dirty_nodes);
            Ok(())
        } else {
            Err("Node not found.".to_string())
        }
    }

    async fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String> {
        let edge = self
            .create_edge(
                from_node_id,
                from_output_slot_index,
                to_node_id,
                to_input_slot_index,
            )
            .await?;

        self.add_edge(edge).await
    }

    async fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), String> {
        let edge = self.remove_edge_from_context(&edge_id)?;

        let from_node_id = edge.from_node_id;
        let to_node_id = edge.to_node_id;

        if let Some(node) = self.node_manager.get_node_by_id(&from_node_id).await {
            let mut node_write = node.write().await;
            if let Some(slot) = node_write.get_output_slot_by_index_mut(edge.from_output_slot_index)
            {
                slot.connected_edges.retain(|id| id.clone() != edge_id);
            }
        }

        if let Some(node) = self.node_manager.get_node_by_id(&to_node_id).await {
            let mut node_write = node.write().await;
            if let Some(slot) = node_write.get_input_slot_by_index_mut(edge.to_input_slot_index) {
                slot.connected_edges.retain(|id| id.clone() != edge_id);
            }
        }

        let dirty_nodes = self.collect_dirty_nodes(vec![from_node_id, to_node_id])?;
        self.mark_dirty_nodes(dirty_nodes);

        Ok(())
    }

    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, &'static str> {
        let context = self.context.lock().map_err(|_| "Failed to lock context")?;
        context.edges.get(&edge_id).cloned().ok_or("Edge not found")
    }

    async fn get_node_by_id(&self, node_id: &NodeId) -> Option<NodeEntity> {
        self.node_manager
            .get_node_by_id(node_id)
            .await
            .map(|node| Arc::clone(&node))
    }

    async fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId> {
        self.node_manager.get_node_ids_by_type::<T>().await
    }

    async fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)> {
        self.node_manager.get_nodes_by_ids(ids).await
    }

    async fn get_output_value(
        &self,
        node_id: &NodeId,
        output_slot_index: usize,
    ) -> Option<SharedData> {
        let node = self.get_node_by_id(node_id).await?;
        let read_node = node.read().await;
        let slot = read_node.outputs().get(output_slot_index)?;

        let context = self.context.lock().ok()?;
        context.outputs.get(&slot.id).cloned()
    }

    async fn set_default_value<T: 'static + NodePrimitive + NodeImpl>(
        &mut self,
        node_id: &NodeId,
        value: Data,
    ) -> Result<(), String> {
        let node = self
            .node_manager
            .get_node_by_id(node_id)
            .await
            .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;

        let mut node_write = node.write().await;
        let downcast_node = node_write.downcast_mut::<T>().ok_or_else(|| {
            format!(
                "Invalid Node: expected implementation of NodePrimitive for {:?}",
                node_id
            )
        })?;

        downcast_node.set_default_value(self.context.clone(), value)?;

        Ok(())
    }
}
