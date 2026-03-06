//! SubGraph node system.
//!
//! A SubGraphNode contains a nested NodeGraph, allowing hierarchical
//! graph composition. External inputs flow through SubGraphInputNode
//! proxy, and internal results exit through SubGraphOutputNode proxy.

pub mod input_proxy;
pub mod output_proxy;

pub use input_proxy::SubGraphInputNode;
pub use output_proxy::SubGraphOutputNode;

use std::sync::Arc;

use crate::{
    Data, DataType, ExecutionContext, InputSlot, NodeCategory, NodeCore, NodeGraph, NodeGraphAPI,
    NodeId, NodeImpl, NodeManager, NodeMeta, NodeValueSetter, OutputSlot, SharedExecutionCache,
    SharedNodeStates, SlotDef,
};

/// A node that contains a nested NodeGraph.
///
/// SubGraphNode acts as a single node in the parent graph while internally
/// containing a full computation graph. Inputs are bridged via
/// SubGraphInputNode, and outputs via SubGraphOutputNode.
///
/// # Data Flow
///
/// ```text
/// Parent Graph                    Internal Graph
/// ┌──────────────────┐
/// │  SubGraphNode    │
/// │  ┌────────────┐  │
/// │  │ InputProxy  ├──┼──> [internal nodes] ──> OutputProxy
/// │  └────────────┘  │                          │
/// │  outputs ◄───────┼──────────────────────────┘
/// └──────────────────┘
/// ```
pub struct SubGraphNode {
    pub node_data: Option<Data>,
    pub inputs: Vec<InputSlot>,
    pub outputs: Vec<OutputSlot>,
    /// The nested graph that this subgraph node manages.
    internal_graph: NodeGraph,
    /// NodeId of the SubGraphInputNode inside the internal graph.
    input_proxy_id: NodeId,
    /// NodeId of the SubGraphOutputNode inside the internal graph.
    output_proxy_id: NodeId,
    /// Label for UI display (e.g., "StairGroup").
    label: String,
    /// Number of proxy outputs each external input maps to.
    /// Normally 1 (1:1 mapping). For multi-input ports like Baseline, this is N.
    input_proxy_counts: Vec<usize>,
}

impl std::fmt::Debug for SubGraphNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubGraphNode")
            .field("inputs", &self.inputs.len())
            .field("outputs", &self.outputs.len())
            .field("input_proxy_id", &self.input_proxy_id)
            .field("output_proxy_id", &self.output_proxy_id)
            .field("label", &self.label)
            .field("input_proxy_counts", &self.input_proxy_counts)
            .finish()
    }
}

impl SubGraphNode {
    /// Create a new SubGraphNode with empty internal graph.
    pub fn new(label: impl Into<String>) -> Result<Self, String> {
        let mut internal_graph = NodeGraph::new()?;
        let input_proxy_id = internal_graph.create_node::<SubGraphInputNode>()?;
        let output_proxy_id = internal_graph.create_node::<SubGraphOutputNode>()?;

        Ok(Self {
            node_data: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            internal_graph,
            input_proxy_id,
            output_proxy_id,
            label: label.into(),
            input_proxy_counts: Vec::new(),
        })
    }

    /// Get a reference to the internal graph.
    pub fn internal_graph(&self) -> &NodeGraph {
        &self.internal_graph
    }

    /// Get a mutable reference to the internal graph.
    pub fn internal_graph_mut(&mut self) -> &mut NodeGraph {
        &mut self.internal_graph
    }

    /// Get the NodeId of the input proxy node inside the internal graph.
    pub fn input_proxy_id(&self) -> NodeId {
        self.input_proxy_id
    }

    /// Get the NodeId of the output proxy node inside the internal graph.
    pub fn output_proxy_id(&self) -> NodeId {
        self.output_proxy_id
    }

    /// Get the display label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Set the display label.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    /// Add an input slot to this subgraph node and a corresponding
    /// output slot on the internal SubGraphInputNode.
    pub fn add_input(&mut self, slot: InputSlot) -> Result<(), String> {
        let data_type = slot.data_type;
        let label = slot.label;
        self.inputs.push(slot);
        self.input_proxy_counts.push(1);

        // Add corresponding output on the input proxy (with same label)
        let proxy = self
            .internal_graph
            .get_node_by_id(&self.input_proxy_id)
            .ok_or("Input proxy not found")?;
        let mut write_proxy = proxy.write().map_err(|e| e.to_string())?;
        let output_slot = OutputSlot {
            label,
            data_type,
            ..Default::default()
        };
        write_proxy.outputs_mut().push(output_slot);
        Ok(())
    }

    /// Add a single external input port that maps to multiple proxy outputs.
    ///
    /// Used for ports like "Baseline" where N edges connect to one input slot
    /// and each edge value is distributed to a separate proxy output inside
    /// the internal graph.
    pub fn add_multi_input(
        &mut self,
        slot: InputSlot,
        proxy_count: usize,
        proxy_data_type: DataType,
        proxy_label: &'static str,
    ) -> Result<(), String> {
        self.inputs.push(slot);
        self.input_proxy_counts.push(proxy_count);

        // Add proxy_count output slots on the input proxy
        let proxy = self
            .internal_graph
            .get_node_by_id(&self.input_proxy_id)
            .ok_or("Input proxy not found")?;
        let mut write_proxy = proxy.write().map_err(|e| e.to_string())?;
        for _ in 0..proxy_count {
            write_proxy.outputs_mut().push(OutputSlot {
                label: proxy_label,
                data_type: proxy_data_type,
                ..Default::default()
            });
        }
        Ok(())
    }

    /// Remove the last input slot from this subgraph node and the
    /// corresponding output slot(s) from the internal SubGraphInputNode.
    ///
    /// Returns an error if there are no inputs to remove.
    pub fn remove_input(&mut self, index: usize) -> Result<(), String> {
        if index >= self.inputs.len() {
            return Err(format!(
                "Input index {} out of range (have {})",
                index,
                self.inputs.len()
            ));
        }

        // Calculate the proxy output range for this input
        let proxy_start: usize = self.input_proxy_counts[..index].iter().sum();
        let proxy_count = self.input_proxy_counts[index];

        // Remove from proxy outputs
        let proxy = self
            .internal_graph
            .get_node_by_id(&self.input_proxy_id)
            .ok_or("Input proxy not found")?;
        let mut write_proxy = proxy.write().map_err(|e| e.to_string())?;
        for _ in 0..proxy_count {
            if proxy_start < write_proxy.outputs().len() {
                write_proxy.outputs_mut().remove(proxy_start);
            }
        }
        drop(write_proxy);

        // Remove from self
        self.inputs.remove(index);
        self.input_proxy_counts.remove(index);

        Ok(())
    }

    /// Remove an output slot from this subgraph node and the
    /// corresponding input slot from the internal SubGraphOutputNode.
    ///
    /// Returns an error if the index is out of range.
    pub fn remove_output(&mut self, index: usize) -> Result<(), String> {
        if index >= self.outputs.len() {
            return Err(format!(
                "Output index {} out of range (have {})",
                index,
                self.outputs.len()
            ));
        }

        // Remove from proxy inputs
        let proxy = self
            .internal_graph
            .get_node_by_id(&self.output_proxy_id)
            .ok_or("Output proxy not found")?;
        let mut write_proxy = proxy.write().map_err(|e| e.to_string())?;
        if index < write_proxy.inputs().len() {
            write_proxy.inputs_mut().remove(index);
        }
        drop(write_proxy);

        // Remove from self
        self.outputs.remove(index);

        Ok(())
    }

    /// Add an output slot to this subgraph node and a corresponding
    /// input slot on the internal SubGraphOutputNode.
    pub fn add_output(&mut self, slot: OutputSlot) -> Result<(), String> {
        let data_type = slot.data_type;
        let label = slot.label;
        self.outputs.push(slot);

        // Add corresponding input on the output proxy (with same label)
        let proxy = self
            .internal_graph
            .get_node_by_id(&self.output_proxy_id)
            .ok_or("Output proxy not found")?;
        let mut write_proxy = proxy.write().map_err(|e| e.to_string())?;
        let input_slot = InputSlot {
            label,
            data_type,
            ..Default::default()
        };
        write_proxy.inputs_mut().push(input_slot);
        Ok(())
    }

    /// Set the default value for an input port.
    /// Used for promoted properties - when not connected, this value is injected.
    pub fn set_input_default(&mut self, input_index: usize, data: Data) -> Result<(), String> {
        let slot = self
            .inputs
            .get_mut(input_index)
            .ok_or_else(|| format!("Input index {} out of range", input_index))?;
        slot.set_default_value(data)
    }

    /// Inject external input data into the internal input proxy's output cache.
    ///
    /// Uses a cursor to map external inputs to proxy outputs:
    /// - For normal inputs (count=1): one value → one proxy output (with default fallback)
    /// - For multi-inputs (count>1): N edge values → N proxy outputs (no default)
    ///
    /// Edge topology is read from `parent_node_states`; output values from `parent_cache`.
    fn inject_inputs(
        &self,
        parent_cache: &SharedExecutionCache,
        parent_node_states: &SharedNodeStates,
    ) -> Result<(), String> {
        let proxy = self
            .internal_graph
            .get_node_by_id(&self.input_proxy_id)
            .ok_or("Input proxy not found")?;
        let read_proxy = proxy.read().map_err(|e| e.to_string())?;
        let internal_cache = self.internal_graph.shared_cache();

        let mut proxy_cursor: usize = 0;

        for (idx, input_slot) in self.inputs.iter().enumerate() {
            let count = self.input_proxy_counts.get(idx).copied().unwrap_or(1);

            // Collect all edge values for this input slot
            // Edges from parent NodeStates, output data from parent cache
            let ns = parent_node_states.read().map_err(|e| e.to_string())?;
            let parent_cache_read = parent_cache.read()?;
            let mut data_values = Vec::new();
            for edge_id in ns.edges_for_input(&input_slot.id) {
                if let Some(edge) = ns.get_edge(edge_id) {
                    if let Some(data) = parent_cache_read.outputs.get(&edge.from_output_slot_id) {
                        data_values.push(data.share());
                    }
                }
            }
            drop(parent_cache_read);
            drop(ns);

            if count == 1 {
                // Normal 1:1 mapping with default value fallback
                if let Some(data) = data_values.into_iter().next() {
                    if let Some(proxy_output_slot) = read_proxy.outputs().get(proxy_cursor) {
                        let mut cache = internal_cache.lock()?;
                        cache.outputs.insert(proxy_output_slot.id, data);
                    }
                } else if let Some(default_ref) = input_slot.default_value.as_ref() {
                    if let Ok(default_data) = Data::from_any(Arc::clone(default_ref)) {
                        if let Some(proxy_output_slot) = read_proxy.outputs().get(proxy_cursor) {
                            let mut cache = internal_cache.lock()?;
                            cache.outputs.insert(proxy_output_slot.id, default_data);
                        }
                    }
                }
            } else {
                // Multi-input: distribute N edge values to N proxy outputs
                let distribute_count = data_values.len().min(count);
                for i in 0..distribute_count {
                    if let Some(proxy_output_slot) = read_proxy.outputs().get(proxy_cursor + i) {
                        let mut cache = internal_cache.lock()?;
                        cache
                            .outputs
                            .insert(proxy_output_slot.id, data_values[i].share());
                    }
                }
            }

            proxy_cursor += count;
        }

        Ok(())
    }

    /// Execute the SubGraphNode with mutable access.
    ///
    /// Called by the parent graph's execution engine, which holds a write lock
    /// on the node entity. This allows mutable access to the internal graph
    /// for execution, which is not possible from `NodeImpl::execute(&self)`.
    ///
    /// Performs the full execution cycle:
    /// 1. Inject external inputs into the internal input proxy
    /// 2. Execute the internal graph
    /// 3. Collect outputs from the internal output proxy
    pub async fn execute_internal(
        &self,
        parent_cache: SharedExecutionCache,
        parent_node_states: SharedNodeStates,
    ) -> Result<(), String> {
        self.inject_inputs(&parent_cache, &parent_node_states)?;
        // Mark all internal nodes dirty so they execute.
        // This is needed because the internal graph doesn't know that
        // its proxy inputs changed.
        self.internal_graph.mark_all_nodes_dirty();
        let _changes = self.internal_graph.execute().await?;
        self.collect_outputs(&parent_cache)?;
        Ok(())
    }

    /// Read results from the internal output proxy and write to this node's outputs.
    ///
    /// Edge topology is read from internal graph's NodeStates; output values
    /// from internal cache. Results are written to parent cache.
    fn collect_outputs(&self, parent_cache: &SharedExecutionCache) -> Result<(), String> {
        let internal_ns = self
            .internal_graph
            .node_states()
            .read()
            .map_err(|e| e.to_string())?;
        let internal_cache = self.internal_graph.shared_cache();

        for (idx, output_slot) in self.outputs.iter().enumerate() {
            // Find edges connected to the output proxy's input slot at this index
            if let Some(slot_state) = internal_ns.input_slot(&self.output_proxy_id, idx) {
                let cache_read = internal_cache.read()?;
                for edge_id in &slot_state.connected_edges {
                    if let Some(edge) = internal_ns.get_edge(edge_id) {
                        if let Some(data) = cache_read.outputs.get(&edge.from_output_slot_id) {
                            let data = data.share();
                            drop(cache_read);
                            // Write to parent cache output
                            let mut parent_write = parent_cache.lock()?;
                            parent_write.outputs.insert(output_slot.id, data);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl NodeMeta for SubGraphNode {
    const NAME: &'static str = "SubGraph";
    const CATEGORY: NodeCategory = NodeCategory::Utility;
    // Dynamic slots - these constants are not used for SubGraphNode
    const INPUTS: &'static [SlotDef] = &[];
    const OUTPUTS: &'static [SlotDef] = &[];
}

#[async_trait::async_trait]
impl NodeImpl for SubGraphNode {
    async fn execute(&self, _ctx: ExecutionContext) -> Result<(), String> {
        // SubGraphNode execution is handled by execute_internal() which is called
        // directly by the execution engine with the parent cache.
        // This NodeImpl::execute() is not called for SubGraphNode.
        Ok(())
    }
}

// Manual implementation of NodeCore (can't use register_nodes! with dynamic slots)
impl NodeCore for SubGraphNode {
    fn node_name(&self) -> &'static str {
        "SubGraph"
    }

    fn node_data(&self) -> Option<Data> {
        self.node_data.as_ref().map(|d| d.share())
    }

    fn node_data_mut(&mut self) -> &mut Option<Data> {
        &mut self.node_data
    }

    fn inputs(&self) -> &Vec<InputSlot> {
        &self.inputs
    }

    fn inputs_mut(&mut self) -> &mut Vec<InputSlot> {
        &mut self.inputs
    }

    fn outputs(&self) -> &Vec<OutputSlot> {
        &self.outputs
    }

    fn outputs_mut(&mut self) -> &mut Vec<OutputSlot> {
        &mut self.outputs
    }

    fn register_in(manager: &mut NodeManager) -> Result<(), String> {
        let factory = Arc::new(|| {
            SubGraphNode::new("SubGraph").map(|node| {
                Arc::new(std::sync::RwLock::new(node)) as crate::NodeEntity
            })
        });
        manager.register_factory::<SubGraphNode>(factory.clone())?;
        manager.register_factory_with_name("SubGraph", factory, None);
        Ok(())
    }
}

impl NodeValueSetter for SubGraphNode {}

// Register the SubGraphNode factory
inventory::submit! {
    crate::NodeRegistrationEntry {
        register: SubGraphNode::register_in,
    }
}

inventory::submit! {
    crate::NodeTypeInfo {
        name: "SubGraph",
        category: NodeCategory::Utility,
        inputs: &[],
        outputs: &[],
        default_value: crate::DefaultValue::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeGraphAPI;

    #[tokio::test]
    async fn test_subgraph_creation() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");
        let subgraph_id = graph.create_node::<SubGraphNode>().expect("Failed to create subgraph");

        let node = graph.get_node_by_id(&subgraph_id).expect("Node not found");
        let read = node.read().expect("Lock failed");
        assert_eq!(read.node_name(), "SubGraph");
        assert!(read.inputs().is_empty());
        assert!(read.outputs().is_empty());
    }

    #[tokio::test]
    async fn test_subgraph_has_proxies() {
        let subgraph = SubGraphNode::new("Test").expect("Failed to create subgraph");
        assert!(
            subgraph
                .internal_graph
                .get_node_by_id(&subgraph.input_proxy_id)
                .is_some()
        );
        assert!(
            subgraph
                .internal_graph
                .get_node_by_id(&subgraph.output_proxy_id)
                .is_some()
        );
    }
}
