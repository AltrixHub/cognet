use async_trait::async_trait;
use futures::future::join_all;
use std::sync::Arc;

use crate::{
    node_graph_system::NodeGraphSystem, Data, Edge, EdgeId, NodeEntity, NodeGraph, NodeId,
    NodeImpl, NodeManager,
};

#[async_trait]
pub trait NodeGraphAPI {
    fn new() -> Self;

    fn node_manager(&self) -> &NodeManager;

    fn node_manager_mut(&mut self) -> &mut NodeManager;

    async fn execute(&mut self) -> Result<(), String>;

    async fn remove_node(&mut self, node_id: NodeId) -> Result<(), String>;

    async fn update_node_value<V: 'static + Sync + Send>(
        &mut self,
        node_id: NodeId,
        slot_index: usize,
        value: V,
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

    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, String>;

    async fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<Data>;

    async fn set_default_value<V: 'static + Sync + Send>(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        value: V,
    ) -> Result<(), String>;
}

#[async_trait]
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

        let shared_nodes = self.node_manager.nodes();
        let shared_cache = self.cache.share();

        for level_nodes in sorted_node_levels {
            let tasks = level_nodes
                .into_iter()
                .map(|node_id| {
                    let shared_nodes = shared_nodes.share();
                    let shared_cache = shared_cache.share();
                    tokio::spawn(async move {
                        let nodes = shared_nodes.lock().await;
                        let node = nodes.get(&node_id).cloned();
                        if let Some(node) = node {
                            let node_write = node.read().await;
                            node_write.execute(shared_cache).await?;
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
            self.remove_edges_from_cache(&node_id)?;
            self.mark_dirty_nodes(dirty_nodes);
            Ok(())
        } else {
            Err("Node not found.".to_string())
        }
    }

    async fn update_node_value<V: 'static + Sync + Send>(
        &mut self,
        node_id: NodeId,
        slot_index: usize,
        value: V,
    ) -> Result<(), String> {
        if let Some(node) = self.node_manager.get_node_by_id(&node_id).await {
            let write_node = node.write().await;
            write_node.set_output_value(self.cache.share(), slot_index, Arc::new(value))?;
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
        let edge = self.remove_edge_from_cache(&edge_id)?;

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

    fn get_edge(&self, edge_id: EdgeId) -> Result<Edge, String> {
        let cache = self.cache.lock()?;
        cache
            .edges
            .get(&edge_id)
            .cloned()
            .ok_or(format!("Edge not found: id {:?}", edge_id))
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

    async fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<Data> {
        let node = self.get_node_by_id(node_id).await?;
        let read_node = node.read().await;
        let slot = read_node.outputs().get(output_slot_index)?;

        let cache = self.cache.lock().ok()?;
        cache.outputs.get(&slot.id).map(|data| data.share())
    }

    async fn set_default_value<V: 'static + Sync + Send>(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        value: V,
    ) -> Result<(), String> {
        let node = self
            .node_manager
            .get_node_by_id(node_id)
            .await
            .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;

        let mut node_write = node.write().await;

        node_write.set_default_value(slot_index, Arc::new(value))?;

        Ok(())
    }
}
