use ulid::Ulid;

use crate::{system::NodeGraphSystem, Data, Edge, EdgeId, NodeGraph, NodeId, NodeImpl};

pub trait NodeGraphAPI {
    fn new() -> Self;

    fn execute(&mut self) -> Result<(), String>;

    fn add_node<T: 'static + NodeImpl>(&mut self, node: T) -> NodeId;

    fn remove_node(&mut self, node_id: NodeId) -> Result<(), &'static str>;

    fn update_node<T: 'static + NodeImpl>(
        &mut self,
        node_id: NodeId,
        new_node: T,
    ) -> Result<(), &'static str>;

    fn get_node_by_id(&self, node_id: NodeId) -> Option<&Box<dyn NodeImpl>>;

    fn get_nodes_by_type<T: NodeImpl + 'static>(&self) -> Vec<(NodeId, &T)>;

    fn get_all_nodes(&self) -> Vec<(NodeId, &Box<dyn NodeImpl>)>;

    fn connect_nodes(
        &mut self,
        from_node_id: NodeId,
        from_output_slot_index: usize,
        to_node_id: NodeId,
        to_input_slot_index: usize,
    ) -> Result<EdgeId, String>;

    fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), &'static str>;

    fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge>;

    fn get_output_value(&self, node_id: NodeId, output_slot_index: usize) -> Option<&Data>;
}

impl NodeGraphAPI for NodeGraph {
    fn new() -> Self {
        Self::default()
    }

    fn execute(&mut self) -> Result<(), String> {
        if self.dirty_nodes.is_empty() {
            return Ok(());
        }

        let sorted_nodes = self.topological_sort(&self.dirty_nodes)?;
        for node_id in sorted_nodes {
            if let Some(node) = self.nodes.get(&node_id) {
                node.execute(&mut self.context)?;
            }
        }

        self.dirty_nodes.clear();
        Ok(())
    }

    fn add_node<T: 'static + NodeImpl>(&mut self, node: T) -> NodeId {
        let node_id = Ulid::new();
        let node_box = Box::new(node);
        self.nodes.insert(node_id, node_box);
        self.mark_dirty_nodes(vec![node_id]);
        node_id
    }

    fn remove_node(&mut self, node_id: NodeId) -> Result<(), &'static str> {
        if self.nodes.remove(&node_id).is_some() {
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id]);
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
        if self.nodes.contains_key(&node_id) {
            let node_box = Box::new(new_node);
            self.nodes.insert(node_id, node_box);
            let dirty_nodes = self.collect_dirty_nodes(vec![node_id]);
            self.mark_dirty_nodes(dirty_nodes);
            Ok(())
        } else {
            Err("Node not found.")
        }
    }

    fn connect_nodes(
        &mut self,
        from_node_id: NodeId,
        from_output_slot_index: usize,
        to_node_id: NodeId,
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

        if let Some(node) = self.nodes.get_mut(&from_node_id) {
            if let Some(slot) = node.get_output_slot_by_index_mut(edge.from_output_slot_index) {
                slot.connected_edges.retain(|&id| id != edge_id);
            }
        }

        if let Some(node) = self.nodes.get_mut(&to_node_id) {
            if let Some(slot) = node.get_input_slot_by_index_mut(edge.to_input_slot_index) {
                slot.connected_edges.retain(|&id| id != edge_id);
            }
        }

        let dirty_nodes = self.collect_dirty_nodes(vec![from_node_id, to_node_id]);
        self.mark_dirty_nodes(dirty_nodes);

        Ok(())
    }

    fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge> {
        self.context.edges.get(&edge_id)
    }

    fn get_node_by_id(&self, node_id: NodeId) -> Option<&Box<dyn NodeImpl>> {
        self.nodes.get(&node_id).map(|node| node)
    }

    fn get_nodes_by_type<T: NodeImpl + 'static>(&self) -> Vec<(NodeId, &T)> {
        self.nodes
            .iter()
            .filter_map(|(&id, node)| node.downcast_ref::<T>().map(|typed_node| (id, typed_node)))
            .collect()
    }

    fn get_all_nodes(&self) -> Vec<(NodeId, &Box<dyn NodeImpl>)> {
        self.nodes.iter().map(|(&id, node)| (id, node)).collect()
    }

    fn get_output_value(&self, node_id: NodeId, output_slot_index: usize) -> Option<&Data> {
        let node = self.get_node_by_id(node_id)?;
        let slot = node.outputs().get(output_slot_index)?;
        self.context.outputs.get(&slot.id)
    }
}
