#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{Mutex, MutexGuard, RwLock};

#[cfg(target_arch = "wasm32")]
use async_lock::{Mutex, MutexGuard, RwLock};

use crate::{Data, NodeCore, NodeId, NodeImpl, NodeValueSetter};
use std::{
    any::{type_name, Any, TypeId},
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub trait Node: NodeImpl + NodeValueSetter + NodeCore {}
impl<T: NodeImpl + NodeValueSetter + NodeCore> Node for T {}

pub type NodeEntity = Arc<RwLock<dyn Node>>;
type NodeFactory = Arc<dyn Fn() -> Result<NodeEntity, String> + Send + Sync>;

/// Factory with metadata for name-based node creation.
pub struct NodeFactoryWithMeta {
    /// The factory function to create the node.
    pub factory: NodeFactory,
    /// Default data for the node (if any).
    pub default_data: Option<Data>,
}

#[derive(Default, Debug)]
pub struct SharedNodes {
    inner: Arc<Mutex<HashMap<NodeId, NodeEntity>>>,
}

impl SharedNodes {
    pub fn new(nodes: HashMap<NodeId, NodeEntity>) -> Self {
        SharedNodes {
            inner: Arc::new(Mutex::new(nodes)),
        }
    }

    pub async fn lock(&self) -> MutexGuard<'_, HashMap<NodeId, NodeEntity>> {
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
    /// Name-based registry for dynamic node creation.
    name_registry: HashMap<&'static str, NodeFactoryWithMeta>,
    variants: HashSet<String>,
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

    pub fn variants(&self) -> &HashSet<String> {
        &self.variants
    }

    /// Register a factory by type.
    pub fn register_factory<T: NodeImpl + 'static>(
        &mut self,
        factory: NodeFactory,
    ) -> Result<(), String> {
        let type_id = TypeId::of::<T>();
        let type_name = type_name::<T>();

        let node_name = type_name
            .rsplit("::")
            .next()
            .ok_or_else(|| format!("Failed to extract type name from {:?}", type_name))?
            .to_string();

        self.variants.insert(node_name);

        if self.node_registry.insert(type_id, factory).is_some() {
            Err(format!(
                "Type {:?} is already registered",
                TypeId::of::<T>()
            ))
        } else {
            Ok(())
        }
    }

    /// Register a factory with name and metadata for dynamic creation.
    pub fn register_factory_with_name(
        &mut self,
        name: &'static str,
        factory: NodeFactory,
        default_data: Option<Data>,
    ) {
        self.name_registry
            .insert(name, NodeFactoryWithMeta { factory, default_data });
    }

    /// Create a node by type (compile-time dispatch).
    pub async fn create_node<T: NodeImpl + 'static>(&mut self) -> Result<NodeId, String> {
        if let Some(factory) = self.node_registry.get(&TypeId::of::<T>()) {
            let node = factory()?;
            let node_id = NodeId::new();
            self.node_insert(node_id, node).await;
            Ok(node_id)
        } else {
            Err(format!("{:?} is not registered", TypeId::of::<T>()))
        }
    }

    /// Create a node by name (runtime dispatch).
    ///
    /// Returns the node ID and default data if successful.
    pub async fn create_node_by_name(
        &self,
        name: &str,
    ) -> Result<(NodeId, Option<Data>), String> {
        let factory_meta = self
            .name_registry
            .get(name)
            .ok_or_else(|| format!("Node type '{}' is not registered", name))?;

        let node = (factory_meta.factory)()?;
        let default_data = factory_meta.default_data.as_ref().map(|d| d.share());
        let node_id = NodeId::new();
        self.node_insert(node_id, node).await;

        Ok((node_id, default_data))
    }

    /// Check if a node type is registered by name.
    pub fn is_registered(&self, name: &str) -> bool {
        self.name_registry.contains_key(name)
    }

    /// Get all registered node type names.
    pub fn registered_names(&self) -> Vec<&'static str> {
        self.name_registry.keys().copied().collect()
    }
}
