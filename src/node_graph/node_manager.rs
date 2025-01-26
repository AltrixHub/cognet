use crate::{impl_node_core, impl_primitive_node_core, AddListNode, NodeId, NodeImpl, NumberNode};
use std::{any::TypeId, collections::HashMap, sync::Arc};

type NodeFactory = Arc<dyn Fn() -> Box<dyn NodeImpl> + Send + Sync>;

#[derive(Default)]
pub struct NodeManager {
    nodes: HashMap<NodeId, Box<dyn NodeImpl>>,
    node_registry: HashMap<TypeId, NodeFactory>,
}

impl_node_core!(AddListNode);
impl_primitive_node_core!(NumberNode);

impl NodeManager {
    pub fn new() -> Self {
        let mut manager = Self::default();
        manager.register::<AddListNode>(Arc::new(|| Box::new(AddListNode::initialize())));
        manager.register::<NumberNode>(Arc::new(|| Box::new(NumberNode::initialize())));
        manager
    }

    pub fn nodes(&self) -> &HashMap<NodeId, Box<dyn NodeImpl>> {
        &self.nodes
    }

    pub fn nodes_mut(&mut self) -> &mut HashMap<NodeId, Box<dyn NodeImpl>> {
        &mut self.nodes
    }

    pub fn register<T>(&mut self, factory: NodeFactory)
    where
        T: NodeImpl + 'static,
    {
        self.node_registry.insert(TypeId::of::<T>(), factory);
    }

    pub fn create_node<T>(&mut self) -> Result<NodeId, String>
    where
        T: NodeImpl + 'static,
    {
        if let Some(factory) = self.node_registry.get(&TypeId::of::<T>()) {
            let node = factory();
            let node_id = NodeId::new();
            self.nodes.insert(node_id.clone(), node);
            Ok(node_id)
        } else {
            Err(format!("{:?} is not registered", TypeId::of::<T>()))
        }
    }
}
