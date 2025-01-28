use tokio::sync::{Mutex, MutexGuard, RwLock};

use crate::{impl_node_core, impl_primitive_node_core, AddListNode, NodeId, NodeImpl, NumberNode};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};

pub type NodeEntity = Arc<RwLock<dyn NodeImpl>>;
type NodeFactory = Arc<dyn Fn() -> NodeEntity + Send + Sync>;

#[derive(Default)]
pub struct SharedNodes {
    inner: Arc<Mutex<HashMap<NodeId, NodeEntity>>>,
}

impl SharedNodes {
    pub fn new(nodes: HashMap<NodeId, NodeEntity>) -> Self {
        SharedNodes {
            inner: Arc::new(Mutex::new(nodes)),
        }
    }

    pub async fn lock(&self) -> MutexGuard<HashMap<NodeId, NodeEntity>> {
        self.inner.lock().await
    }

    pub fn share(&self) -> Self {
        SharedNodes {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[derive(Default)]
pub struct NodeManager {
    nodes: SharedNodes,
    node_registry: HashMap<TypeId, NodeFactory>,
}

impl_node_core!(AddListNode);
impl_primitive_node_core!(NumberNode);

impl NodeManager {
    pub fn new() -> Self {
        let mut manager = Self::default();
        manager.register::<AddListNode>(Arc::new(|| {
            Arc::new(RwLock::new(AddListNode::initialize()))
        }));
        manager
            .register::<NumberNode>(Arc::new(|| Arc::new(RwLock::new(NumberNode::initialize()))));
        manager
    }

    pub fn nodes(&self) -> SharedNodes {
        self.nodes.share()
    }

    pub async fn get_node_by_id(&self, id: &NodeId) -> Option<NodeEntity> {
        let nodes = self.nodes.lock().await;
        nodes.get(id).cloned()
    }

    pub async fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)> {
        let nodes = self.nodes.lock().await;

        ids.into_iter()
            .filter_map(|id| {
                nodes.get(&id).map(|node| (id, Arc::clone(node))) // 指定されたNodeIdのNodeEntityを取得
            })
            .collect()
    }

    pub async fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId> {
        let nodes = self.nodes.lock().await;

        let mut result: Vec<NodeId> = Vec::new();

        for (id, node) in nodes.iter() {
            if TypeId::of::<T>() == node.type_id() {
                result.push(id.clone());
            }
        }

        result
    }

    pub async fn contains_key(&self, id: &NodeId) -> bool {
        let nodes = self.nodes.lock().await;
        nodes.contains_key(id)
    }

    pub async fn node_insert(&self, node_id: NodeId, node: NodeEntity) -> Option<NodeEntity> {
        let mut nodes = self.nodes.lock().await;
        nodes.insert(node_id.clone(), node)
    }

    pub async fn node_remove(&self, node_id: &NodeId) -> Option<NodeEntity> {
        let mut nodes = self.nodes.lock().await;
        nodes.remove(node_id)
    }

    pub fn register<T>(&mut self, factory: NodeFactory)
    where
        T: NodeImpl + 'static,
    {
        self.node_registry.insert(TypeId::of::<T>(), factory);
    }

    pub async fn create_node<T>(&mut self) -> Result<NodeId, String>
    where
        T: NodeImpl + 'static,
    {
        if let Some(factory) = self.node_registry.get(&TypeId::of::<T>()) {
            let node = factory();
            let node_id = NodeId::new();
            self.node_insert(node_id.clone(), node).await;
            Ok(node_id)
        } else {
            Err(format!("{:?} is not registered", TypeId::of::<T>()))
        }
    }
}
