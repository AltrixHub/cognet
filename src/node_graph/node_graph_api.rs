#[cfg(not(target_arch = "wasm32"))]
use tokio::task;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_rayon::init_thread_pool;

use async_trait::async_trait;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::{
    node_graph_system::NodeGraphSystem, Data, Edge, EdgeId, NodeEntity, NodeGraph, NodeId,
    NodeImpl, NodeManager,
};

#[async_trait]
pub trait NodeGraphAPI {
    fn new() -> Result<Self, String>
    where
        Self: Sized;

    fn node_manager(&self) -> &NodeManager;

    fn node_manager_mut(&mut self) -> &mut NodeManager;

    async fn execute(&mut self) -> Result<(), String>;

    async fn remove_node(&mut self, node_id: NodeId) -> Result<(), String>;

    async fn update_node_data(&mut self, node_id: &NodeId, data: Data) -> Result<(), String>;

    async fn update_input_slot_default_data(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        data: Data,
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
}

#[async_trait]
impl NodeGraphAPI for NodeGraph {
    fn new() -> Result<Self, String> {
        Ok(Self {
            node_manager: NodeManager::new()?,
            cache: Default::default(),
            dirty_nodes: Default::default(),
        })
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

        #[cfg(target_arch = "wasm32")]
        {
            init_thread_pool(4).await.unwrap();

            for level_nodes in sorted_node_levels {
                let futures: Vec<_> = level_nodes
                    .into_iter()
                    .map(|node_id| {
                        let shared_nodes = shared_nodes.clone();
                        let shared_cache = shared_cache.clone();
                        async move {
                            let nodes = shared_nodes.lock().await;
                            if let Some(node) = nodes.get(&node_id) {
                                let node_read = node.read().await;
                                node_read.execute(shared_cache.clone()).await
                            } else {
                                Ok(())
                            }
                        }
                    })
                    .collect();

                futures::future::join_all(futures)
                    .await
                    .into_iter()
                    .collect::<Result<(), _>>()?;
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let rt_handle = Arc::new(Handle::current());
            for level_nodes in sorted_node_levels {
                let results: Vec<Result<(), String>> = task::spawn_blocking({
                    let shared_nodes = shared_nodes.clone();
                    let shared_cache = shared_cache.clone();
                    let rt_handle = Arc::clone(&rt_handle);
                    move || {
                        level_nodes
                            .into_par_iter()
                            .map(|node_id| {
                                rt_handle.block_on(async {
                                    let nodes = shared_nodes.lock().await;
                                    if let Some(node) = nodes.get(&node_id) {
                                        let node_read = node.read().await;
                                        node_read.execute(shared_cache.clone()).await
                                    } else {
                                        Ok(())
                                    }
                                })
                            })
                            .collect()
                    }
                })
                .await
                .map_err(|e| e.to_string())?;

                for res in results {
                    res?;
                }
            }
        }

        self.dirty_nodes.clear();
        Ok(())
    }

    async fn remove_node(&mut self, node_id: NodeId) -> Result<(), String> {
        if self.node_manager.node_remove(&node_id).await.is_some() {
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id])?;
            self.remove_edges_from_cache(&node_id)?;
            self.mark_dirty_nodes(dirty_nodes);
            Ok(())
        } else {
            Err("Node not found.".to_string())
        }
    }

    async fn update_node_data(&mut self, node_id: &NodeId, data: Data) -> Result<(), String> {
        let node = self
            .node_manager
            .get_node_by_id(node_id)
            .await
            .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;
        let mut write_node = node.write().await;
        write_node.set_node_data(data)?;
        let dirty_nodes = self.collect_dirty_nodes(vec![*node_id])?;
        self.mark_dirty_nodes(dirty_nodes);
        Ok(())
    }

    async fn update_input_slot_default_data(
        &mut self,
        node_id: &NodeId,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String> {
        let node = self
            .node_manager
            .get_node_by_id(node_id)
            .await
            .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;
        let mut write_node = node.write().await;
        write_node.set_input_slot_default_data(slot_index, data)?;
        let dirty_nodes = self.collect_dirty_nodes(vec![*node_id])?;
        self.mark_dirty_nodes(dirty_nodes);
        Ok(())
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
                slot.connected_edges.retain(|id| *id != edge_id);
            }
        }

        if let Some(node) = self.node_manager.get_node_by_id(&to_node_id).await {
            let mut node_write = node.write().await;
            if let Some(slot) = node_write.get_input_slot_by_index_mut(edge.to_input_slot_index) {
                slot.connected_edges.retain(|id| *id != edge_id);
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
}
