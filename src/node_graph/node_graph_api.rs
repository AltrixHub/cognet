use crate::{
    node_graph_system::NodeGraphSystem, Data, Edge, EdgeId, NodeGraph, NodeId, NodeImpl,
    NodeManager, NodePrimitive,
};

pub trait NodeGraphAPI {
    fn new() -> Self;

    fn node_manager(&self) -> &NodeManager;

    fn node_manager_mut(&mut self) -> &mut NodeManager;

    fn execute(&mut self) -> Result<(), String>;

    fn remove_node(&mut self, node_id: NodeId) -> Result<(), &'static str>;

    fn update_node<T: 'static + NodeImpl>(
        &mut self,
        node_id: NodeId,
        new_node: T,
    ) -> Result<(), &'static str>;

    fn get_node_by_id(&self, node_id: &NodeId) -> Option<&Box<dyn NodeImpl>>;

    fn get_nodes_by_type<T: NodeImpl + 'static>(&self) -> Vec<(&NodeId, &T)>;

    fn get_all_nodes(&self) -> Vec<(&NodeId, &Box<dyn NodeImpl>)>;

    fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String>;

    fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), &'static str>;

    fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge>;

    fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<&Data>;

    fn set_default_value<T: 'static + NodePrimitive + NodeImpl>(
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

    fn execute(&mut self) -> Result<(), String> {
        if self.dirty_nodes.is_empty() {
            return Ok(());
        }

        let sorted_nodes = self.topological_sort(&self.dirty_nodes)?;
        for node_id in sorted_nodes {
            if let Some(node) = self.node_manager.nodes().get(&node_id) {
                node.execute(&mut self.context)?;
            }
        }

        self.dirty_nodes.clear();
        Ok(())
    }

    fn remove_node(&mut self, node_id: NodeId) -> Result<(), &'static str> {
        if self.node_manager.nodes_mut().remove(&node_id).is_some() {
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id.clone()]);
            self.context
                .edges
                .retain(|_, edge| edge.from_node_id != node_id && edge.to_node_id != node_id);
            self.mark_dirty_nodes(dirty_nodes);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    fn update_node<T: 'static + NodeImpl>(
        &mut self,
        node_id: NodeId,
        new_node: T,
    ) -> Result<(), &'static str> {
        if self.node_manager.nodes().contains_key(&node_id) {
            let node_box = Box::new(new_node);
            self.node_manager
                .nodes_mut()
                .insert(node_id.clone(), node_box);
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id.clone()]);
            self.mark_dirty_nodes(dirty_nodes);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    fn connect_nodes(
        &mut self,
        from_node_id: &NodeId,
        from_output_slot_index: usize,
        to_node_id: &NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String> {
        let edge = self.create_edge(
            from_node_id,
            from_output_slot_index,
            to_node_id,
            to_input_slot_index,
        )?;

        self.add_edge(edge)
    }

    fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), &'static str> {
        let edge = self
            .context
            .edges
            .remove(&edge_id)
            .ok_or("Edge does not exist")?;

        let from_node_id = edge.from_node_id;
        let to_node_id = edge.to_node_id;

        if let Some(node) = self.node_manager.nodes_mut().get_mut(&from_node_id) {
            if let Some(slot) = node.get_output_slot_by_index_mut(edge.from_output_slot_index) {
                slot.connected_edges.retain(|id| id.clone() != edge_id);
            }
        }

        if let Some(node) = self.node_manager.nodes_mut().get_mut(&to_node_id) {
            if let Some(slot) = node.get_input_slot_by_index_mut(edge.to_input_slot_index) {
                slot.connected_edges.retain(|id| id.clone() != edge_id);
            }
        }

        let dirty_nodes = self.collect_dirty_nodes(vec![from_node_id, to_node_id]);
        self.mark_dirty_nodes(dirty_nodes);

        Ok(())
    }

    fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge> {
        self.context.edges.get(&edge_id)
    }

    fn get_node_by_id(&self, node_id: &NodeId) -> Option<&Box<dyn NodeImpl>> {
        self.node_manager.nodes().get(node_id).map(|node| node)
    }

    fn get_nodes_by_type<T: NodeImpl + 'static>(&self) -> Vec<(&NodeId, &T)> {
        self.node_manager
            .nodes()
            .iter()
            .filter_map(|(id, node)| node.downcast_ref::<T>().map(|typed_node| (id, typed_node)))
            .collect()
    }

    fn get_all_nodes(&self) -> Vec<(&NodeId, &Box<dyn NodeImpl>)> {
        self.node_manager
            .nodes()
            .iter()
            .map(|(id, node)| (id, node))
            .collect()
    }

    fn get_output_value(&self, node_id: &NodeId, output_slot_index: usize) -> Option<&Data> {
        let node = self.get_node_by_id(node_id)?;
        let slot = node.outputs().get(output_slot_index)?;
        self.context.outputs.get(&slot.id)
    }

    fn set_default_value<T: 'static + NodePrimitive + NodeImpl>(
        &mut self,
        node_id: &NodeId,
        value: Data,
    ) -> Result<(), String> {
        let (node_manager, context) = self.resources_mut();

        let node = node_manager
            .nodes_mut()
            .get_mut(node_id)
            .ok_or_else(|| format!("Node with ID {:?} not found", node_id))?;

        let downcast_node = node.downcast_mut::<T>().ok_or_else(|| {
            format!(
                "Invalid Node: expected implementation of NodePrimitive for {:?}",
                node_id
            )
        })?;

        downcast_node.set_default_value(context, value)?;

        Ok(())
    }
}
