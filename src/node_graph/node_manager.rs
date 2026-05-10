use std::sync::{Mutex, MutexGuard, RwLock};

use crate::{Data, NodeCore, NodeId, NodeImpl};
use std::{
    any::{type_name, Any, TypeId},
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub trait Node: NodeImpl + NodeCore {}
impl<T: NodeImpl + NodeCore> Node for T {}

pub type NodeEntity = Arc<RwLock<dyn Node>>;
type NodeFactory = Arc<dyn Fn() -> Result<NodeEntity, String> + Send + Sync>;

/// Factory with metadata for name-based node creation.
pub struct NodeFactoryWithMeta {
    /// The factory function to create the node.
    pub factory: NodeFactory,
    /// Default data for the node (if any).
    pub default_data: Option<Data>,
    /// Rust TypeId for type-safe identification.
    pub type_id: Option<TypeId>,
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

    /// Lock the nodes map for access.
    /// Recovers from poisoned locks (caused by panics caught via catch_unwind).
    pub fn lock(&self) -> Result<MutexGuard<'_, HashMap<NodeId, NodeEntity>>, String> {
        match self.inner.lock() {
            Ok(guard) => Ok(guard),
            Err(poisoned) => Ok(poisoned.into_inner()),
        }
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
    /// Names that were leaked into `&'static str` for runtime
    /// registration. Tracked so that
    /// [`restore_factory_registration`] can find a previously-leaked
    /// key after the corresponding `name_registry` entry was removed.
    leaked_names: HashMap<String, &'static str>,
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

    pub fn all_node_ids(&self) -> Vec<NodeId> {
        self.nodes
            .lock()
            .ok()
            .map(|nodes| nodes.keys().copied().collect())
            .unwrap_or_default()
    }

    pub fn get_node_by_id(&self, id: &NodeId) -> Option<NodeEntity> {
        let nodes = self.nodes.lock().ok()?;
        nodes.get(id).cloned()
    }

    pub fn get_nodes_by_ids(&self, ids: Vec<NodeId>) -> Vec<(NodeId, NodeEntity)> {
        let nodes = match self.nodes.lock() {
            Ok(n) => n,
            Err(_) => return Vec::new(),
        };

        ids.into_iter()
            .filter_map(|id| nodes.get(&id).map(|node| (id, Arc::clone(node))))
            .collect()
    }

    pub fn get_node_ids_by_type<T: NodeImpl + 'static>(&self) -> Vec<NodeId> {
        let nodes = match self.nodes.lock() {
            Ok(n) => n,
            Err(_) => return Vec::new(),
        };

        let mut result: Vec<NodeId> = Vec::new();

        for (id, node) in nodes.iter() {
            if TypeId::of::<T>() == node.type_id() {
                result.push(*id);
            }
        }

        result
    }

    pub fn contains_key(&self, id: &NodeId) -> bool {
        let nodes = match self.nodes.lock() {
            Ok(n) => n,
            Err(_) => return false,
        };
        nodes.contains_key(id)
    }

    pub fn node_insert(&self, node_id: NodeId, node: NodeEntity) -> Option<NodeEntity> {
        let mut nodes = self.nodes.lock().ok()?;
        nodes.insert(node_id, node)
    }

    pub fn node_remove(&self, node_id: &NodeId) -> Option<NodeEntity> {
        let mut nodes = self.nodes.lock().ok()?;
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
        type_id: Option<TypeId>,
    ) {
        self.name_registry.insert(
            name,
            NodeFactoryWithMeta {
                factory,
                default_data,
                type_id,
            },
        );
    }

    /// Register a factory with a runtime-built (owned) name. The name
    /// is leaked once into a `&'static str` so it can be inserted into
    /// the same `name_registry` keyed by `&'static str`.
    ///
    /// Intended for callers that build factory names from user-supplied
    /// data at runtime (e.g. per-variant SubGraph factories in the
    /// modeling example's pillar 2). Re-registering an existing name
    /// replaces the entry without leaking again.
    pub fn register_factory_with_name_owned(
        &mut self,
        name: String,
        factory: NodeFactory,
        default_data: Option<Data>,
        type_id: Option<TypeId>,
    ) {
        // Reuse a previously-leaked key for the same name so that the
        // leaked-string footprint is bounded by the count of distinct
        // factory names ever registered (re-registration / unregister +
        // re-register cycles do not leak again).
        let key: &'static str = match self.leaked_names.get(&name) {
            Some(existing) => *existing,
            None => {
                let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
                self.leaked_names.insert(name, leaked);
                leaked
            }
        };
        self.name_registry.insert(
            key,
            NodeFactoryWithMeta {
                factory,
                default_data,
                type_id,
            },
        );
    }

    /// Remove a name-based factory registration. Returns the removed
    /// entry if one was present. Used by callers that need to roll back
    /// a registration after a downstream commit failure.
    pub fn unregister_factory_by_name(&mut self, name: &str) -> Option<NodeFactoryWithMeta> {
        self.name_registry.remove(name)
    }

    /// Re-insert a previously-removed registration entry under the same
    /// (already-leaked) name. The companion to
    /// [`unregister_factory_by_name`] for atomic rollback paths.
    ///
    /// If the name is unknown (never been leaked / registered), the
    /// entry is dropped — callers must therefore only restore entries
    /// for names that were obtained via a prior `take` on the same
    /// manager.
    pub fn restore_factory_registration(
        &mut self,
        name: &str,
        entry: NodeFactoryWithMeta,
    ) -> Result<(), NodeFactoryWithMeta> {
        // Look up either the live key (still in name_registry) or the
        // leaked-name table (key was removed by a prior unregister).
        let key: Option<&'static str> = self
            .name_registry
            .get_key_value(name)
            .map(|(k, _)| *k)
            .or_else(|| self.leaked_names.get(name).copied());
        match key {
            Some(k) => {
                self.name_registry.insert(k, entry);
                Ok(())
            }
            None => Err(entry),
        }
    }

    /// Read-only lookup for a name-based factory registration. Used by
    /// rollback paths that need to capture the previous entry's
    /// metadata before overwriting.
    pub fn factory_meta_by_name(&self, name: &str) -> Option<&NodeFactoryWithMeta> {
        self.name_registry.get(name)
    }

    /// Create a node by type (compile-time dispatch).
    pub fn create_node<T: NodeImpl + 'static>(&mut self) -> Result<NodeId, String> {
        if let Some(factory) = self.node_registry.get(&TypeId::of::<T>()) {
            let node = factory()?;
            let node_id = NodeId::new();
            self.node_insert(node_id, node);
            Ok(node_id)
        } else {
            Err(format!("{:?} is not registered", TypeId::of::<T>()))
        }
    }

    /// Create a node by type with a pre-generated NodeId (compile-time dispatch).
    ///
    /// Same as `create_node()` but uses the caller-provided `id` instead of
    /// generating a fresh one. Allows callers to know the NodeId before the
    /// node is actually created (e.g., for declarative change queues).
    pub fn create_node_with_id<T: NodeImpl + 'static>(
        &mut self,
        id: NodeId,
    ) -> Result<NodeId, String> {
        if let Some(factory) = self.node_registry.get(&TypeId::of::<T>()) {
            let node = factory()?;
            self.node_insert(id, node);
            Ok(id)
        } else {
            Err(format!("{:?} is not registered", TypeId::of::<T>()))
        }
    }

    /// Create a node by name (runtime dispatch).
    ///
    /// Returns the node ID, default data, and TypeId if successful.
    pub fn create_node_by_name(
        &self,
        name: &str,
    ) -> Result<(NodeId, Option<Data>, Option<TypeId>), String> {
        let factory_meta = self
            .name_registry
            .get(name)
            .ok_or_else(|| format!("Node type '{}' is not registered", name))?;

        let node = (factory_meta.factory)()?;
        let default_data = factory_meta.default_data.as_ref().map(|d| d.share());
        let type_id = factory_meta.type_id;
        let node_id = NodeId::new();
        self.node_insert(node_id, node);

        Ok((node_id, default_data, type_id))
    }

    /// Create a node by name with a pre-generated NodeId (runtime dispatch).
    ///
    /// Same as `create_node_by_name()` but uses the caller-provided `id` instead
    /// of generating a fresh one. This allows callers to know the NodeId before
    /// the node is actually created (e.g., for declarative change queues).
    pub fn create_node_by_name_with_id(
        &self,
        id: NodeId,
        name: &str,
    ) -> Result<(NodeId, Option<Data>, Option<TypeId>), String> {
        let factory_meta = self
            .name_registry
            .get(name)
            .ok_or_else(|| format!("Node type '{}' is not registered", name))?;

        let node = (factory_meta.factory)()?;
        let default_data = factory_meta.default_data.as_ref().map(|d| d.share());
        let type_id = factory_meta.type_id;
        self.node_insert(id, node);

        Ok((id, default_data, type_id))
    }

    /// Check if a node type is registered by name.
    pub fn is_registered(&self, name: &str) -> bool {
        self.name_registry.contains_key(name)
    }

    /// Convenience alias for [`is_registered`]. Reads more naturally at
    /// call sites that ask "does this factory exist" rather than "is
    /// this name registered".
    pub fn has_factory(&self, name: &str) -> bool {
        self.is_registered(name)
    }

    /// Identity probe for a name-based factory registration. Returns
    /// the address of the underlying factory closure (`Arc::as_ptr`)
    /// when registered. Two factories registered under different names
    /// will compare unequal; re-registering a name with a fresh closure
    /// changes the returned pointer.
    ///
    /// Pillar-2 of the modeling example uses this to assert that two
    /// variants registered under different names hold independent
    /// closures (per `factory_id != factory_id`).
    pub fn factory_id(&self, name: &str) -> Option<*const ()> {
        self.name_registry
            .get(name)
            .map(|meta| Arc::as_ptr(&meta.factory) as *const ())
    }

    /// Get all registered node type names.
    pub fn registered_names(&self) -> Vec<&'static str> {
        self.name_registry.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeCore, NodeImpl};

    /// Trivial node for register/unregister tests.
    #[derive(Debug)]
    struct Probe;
    impl NodeImpl for Probe {
        fn execute_sync(&self, _ctx: crate::ExecutionContext) -> Result<(), String> {
            Ok(())
        }
    }
    impl NodeCore for Probe {
        fn node_name(&self) -> &'static str {
            "Probe"
        }
        fn register_in(_manager: &mut NodeManager) -> Result<(), String> {
            Ok(())
        }
    }

    fn probe_factory() -> NodeFactory {
        Arc::new(|| Ok(Arc::new(RwLock::new(Probe)) as NodeEntity))
    }

    #[test]
    fn register_factory_with_name_owned_inserts_runtime_name() {
        let mut nm = NodeManager::default();
        nm.register_factory_with_name_owned(
            "VariantFactory_x_runtime".to_string(),
            probe_factory(),
            None,
            None,
        );
        assert!(nm.has_factory("VariantFactory_x_runtime"));
        assert!(nm.factory_id("VariantFactory_x_runtime").is_some());
    }

    #[test]
    fn register_factory_with_name_owned_does_not_releak_on_replace() {
        let mut nm = NodeManager::default();
        nm.register_factory_with_name_owned("Foo".to_string(), probe_factory(), None, None);
        let names_after_first = nm.registered_names().len();
        nm.register_factory_with_name_owned("Foo".to_string(), probe_factory(), None, None);
        // Replacement: same key reused, no extra entry.
        assert_eq!(nm.registered_names().len(), names_after_first);
    }

    #[test]
    fn unregister_factory_by_name_returns_entry_and_removes() {
        let mut nm = NodeManager::default();
        nm.register_factory_with_name_owned("Bar".to_string(), probe_factory(), None, None);
        let removed = nm.unregister_factory_by_name("Bar");
        assert!(removed.is_some());
        assert!(!nm.has_factory("Bar"));
    }

    #[test]
    fn restore_factory_registration_only_succeeds_for_known_name() {
        let mut nm = NodeManager::default();
        nm.register_factory_with_name_owned("Baz".to_string(), probe_factory(), None, None);
        let entry = nm.unregister_factory_by_name("Baz").expect("removed entry");
        // The leaked-key set still contains "Baz" (we keyed on the same
        // string), so restore must succeed even after the name_registry
        // entry was removed.
        assert!(nm.restore_factory_registration("Baz", entry).is_ok());
        assert!(nm.has_factory("Baz"));
    }

    #[test]
    fn restore_factory_registration_rejects_unknown_name() {
        let mut nm = NodeManager::default();
        let entry = NodeFactoryWithMeta {
            factory: probe_factory(),
            default_data: None,
            type_id: None,
        };
        let err = nm
            .restore_factory_registration("never_registered", entry)
            .err();
        assert!(err.is_some(), "restore must reject unknown name");
    }

    #[test]
    fn factory_id_distinguishes_separate_registrations() {
        let mut nm = NodeManager::default();
        nm.register_factory_with_name_owned("A".to_string(), probe_factory(), None, None);
        nm.register_factory_with_name_owned("B".to_string(), probe_factory(), None, None);
        let a = nm.factory_id("A").unwrap();
        let b = nm.factory_id("B").unwrap();
        assert_ne!(a, b);
    }
}
