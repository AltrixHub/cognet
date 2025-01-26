pub mod api;
pub mod system;

pub use api::*;
use ulid::Ulid;

use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{
    impl_node_core, impl_primitive_node_core, AddListNode, Data, Edge, EdgeId, NodeCore, NodeId,
    NodeImpl, NumberNode, OutputSlotId,
};

#[derive(Default, Debug)]
pub struct EvaluationContext {
    pub(crate) edges: HashMap<EdgeId, Edge>,
    pub(crate) outputs: HashMap<OutputSlotId, Data>,
}

pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static + NodeCore> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl dyn NodeImpl {
    pub fn downcast_ref<T: NodeImpl + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    pub fn downcast_mut<T: NodeImpl + 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

type NodeFactory = Arc<dyn Fn() -> Box<dyn NodeImpl> + Send + Sync>;

#[derive(Default)]
pub struct NodeManager {
    nodes: HashMap<NodeId, Box<dyn NodeImpl>>,
    node_registry: HashMap<TypeId, NodeFactory>,
}

impl NodeManager {
    pub fn new() -> Self {
        let mut manager = Self::default();
        manager.register::<AddListNode>(Arc::new(|| Box::new(AddListNode::initialize())));
        manager.register::<NumberNode>(Arc::new(|| Box::new(NumberNode::initialize())));
        manager
    }

    fn nodes(&self) -> &HashMap<NodeId, Box<dyn NodeImpl>> {
        &self.nodes
    }

    fn nodes_mut(&mut self) -> &mut HashMap<NodeId, Box<dyn NodeImpl>> {
        &mut self.nodes
    }

    pub fn register<T>(&mut self, factory: NodeFactory)
    where
        T: NodeImpl + 'static,
    {
        self.node_registry.insert(TypeId::of::<T>(), factory);
    }

    pub fn create_node<T>(&mut self) -> Option<NodeId>
    where
        T: NodeImpl + 'static,
    {
        if let Some(factory) = self.node_registry.get(&TypeId::of::<T>()) {
            let node = factory();
            let node_id = Ulid::new();
            self.nodes.insert(node_id, node);
            Some(node_id)
        } else {
            None
        }
    }
}

impl_node_core!(AddListNode);
impl_primitive_node_core!(NumberNode);

pub struct NodeGraph {
    node_manager: NodeManager,
    context: EvaluationContext,
    dirty_nodes: HashSet<NodeId>,
}

impl Default for NodeGraph {
    fn default() -> Self {
        Self {
            node_manager: NodeManager::new(),
            context: Default::default(),
            dirty_nodes: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{AddListNode, NodePrimitive, NumberNode};

    use super::*;

    #[test]
    fn test_execute() {
        let mut node_graph = NodeGraph::new();

        let node1_id = node_graph.node_manager.create_node::<NumberNode>().unwrap();
        let node2_id = node_graph.node_manager.create_node::<NumberNode>().unwrap();
        let node3_id = node_graph
            .node_manager
            .create_node::<AddListNode>()
            .unwrap();

        let node1 = node_graph
            .node_manager
            .nodes_mut()
            .get_mut(&node1_id)
            .unwrap()
            .downcast_mut::<NumberNode>()
            .unwrap();
        node1
            .set_default_value(&mut node_graph.context, Data::Number(10.))
            .unwrap();
        let node2 = node_graph
            .node_manager
            .nodes_mut()
            .get_mut(&node2_id)
            .unwrap()
            .downcast_mut::<NumberNode>()
            .unwrap();
        node2
            .set_default_value(&mut node_graph.context, Data::Number(20.))
            .unwrap();

        node_graph.connect_nodes(node1_id, 0, node3_id, 0).unwrap();
        node_graph.connect_nodes(node2_id, 0, node3_id, 0).unwrap();

        assert!(node_graph.execute().is_ok());

        let res = node_graph.get_output_value(node3_id, 0).unwrap();
        assert_eq!(res, &Data::Number(30.));
    }
}
