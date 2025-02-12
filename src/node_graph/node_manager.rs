#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{Mutex, MutexGuard, RwLock};

#[cfg(target_arch = "wasm32")]
use async_lock::{Mutex, MutexGuard, RwLock};

use crate::{NodeCore, NodeId, NodeImpl, NodeValueSetter};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};

pub trait Node: NodeImpl + NodeValueSetter + NodeCore {}
impl<T: NodeImpl + NodeValueSetter + NodeCore> Node for T {}

pub type NodeEntity = Arc<RwLock<dyn Node>>;
type NodeFactory = Arc<dyn Fn() -> Result<NodeEntity, String> + Send + Sync>;

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

pub type NodeRegistrationFn = fn(&mut NodeManager) -> Result<(), String>;

pub struct NodeRegistrationEntry {
    pub register: NodeRegistrationFn,
}

inventory::collect!(NodeRegistrationEntry);

impl NodeManager {
    pub fn new() -> Result<Self, String> {
        let mut manager = Self::default();
        for entry in inventory::iter::<NodeRegistrationEntry> {
            (entry.register)(&mut manager)?;
        }
        Ok(manager)
    }

    pub fn nodes(&self) -> SharedNodes {
        self.nodes.share()
    }

    pub async fn get_node_by_id(&self, id: &NodeId) -> Option<NodeEntity> {
        let nodes = self.nodes.lock().await;
        nodes.get(id).map(|node| Arc::clone(node))
    }

    pub async fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)> {
        let nodes = self.nodes.lock().await;

        ids.into_iter()
            .filter_map(|id| nodes.get(&id).map(|node| (id, Arc::clone(node))))
            .collect()
    }

    pub async fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId> {
        let nodes = self.nodes.lock().await;

        let mut result: Vec<NodeId> = Vec::new();

        for (id, node) in nodes.iter() {
            if TypeId::of::<T>() == node.type_id() {
                result.push(*id);
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
        nodes.insert(node_id, node)
    }

    pub async fn node_remove(&self, node_id: &NodeId) -> Option<NodeEntity> {
        let mut nodes = self.nodes.lock().await;
        nodes.remove(node_id)
    }

    pub fn register_factory<T>(&mut self, factory: NodeFactory) -> Result<(), String>
    where
        T: NodeImpl + 'static,
    {
        if self
            .node_registry
            .insert(TypeId::of::<T>(), factory)
            .is_some()
        {
            Err(format!(
                "Type {:?} is already registered",
                TypeId::of::<T>()
            ))
        } else {
            Ok(())
        }
    }

    pub async fn create_node<T>(&mut self) -> Result<NodeId, String>
    where
        T: NodeImpl + 'static,
    {
        if let Some(factory) = self.node_registry.get(&TypeId::of::<T>()) {
            let node = factory()?;
            let node_id = NodeId::new();
            self.node_insert(node_id, node).await;
            Ok(node_id)
        } else {
            Err(format!("{:?} is not registered", TypeId::of::<T>()))
        }
    }
}
