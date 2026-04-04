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
    Data, DataType, Edge, EdgeId, ExecutionContext, NodeCategory, NodeCore, NodeGraph,
    NodeGraphWrite, NodeId, NodeImpl, NodeManager, NodeMeta, SharedExecutionCache,
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
    /// For dynamic inputs, this starts at 0 and is adjusted at execution time.
    input_proxy_counts: Vec<usize>,
    /// Per-slot dynamic input target info. `None` = normal/multi input.
    /// `Some(...)` = dynamic input that auto-resizes proxy outputs at execution time.
    dynamic_input_targets: Vec<Option<DynamicInputTarget>>,
}

/// Target info for a dynamic multi-input port.
///
/// Stores which internal node and slot the proxy outputs should connect to,
/// so that `sync_dynamic_inputs` can automatically create/remove internal
/// edges when the number of external connections changes.
#[derive(Debug, Clone)]
struct DynamicInputTarget {
    /// Internal node to connect proxy outputs to.
    internal_node_id: NodeId,
    /// Input slot index on the internal node (must be multi-connection).
    internal_slot: usize,
    /// Data type for dynamically created proxy output slots.
    proxy_data_type: DataType,
    /// Label for dynamically created proxy output slots.
    proxy_label: &'static str,
}

impl std::fmt::Debug for SubGraphNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubGraphNode")
            .field("inputs", &self.input_proxy_counts.len())
            .field("outputs", &self.output_count())
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
            internal_graph,
            input_proxy_id,
            output_proxy_id,
            label: label.into(),
            input_proxy_counts: Vec::new(),
            dynamic_input_targets: Vec::new(),
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

    /// Number of output slots, derived from the internal OutputProxy's input slot count.
    pub fn output_count(&self) -> usize {
        self.internal_graph
            .node_states()
            .read()
            .map(|ns| ns.input_slot_count(&self.output_proxy_id))
            .unwrap_or(0)
    }

    /// Add an input slot to this subgraph node and a corresponding
    /// output slot on the internal SubGraphInputNode (via NodeStates).
    pub fn add_input(&mut self, label: &'static str, data_type: DataType) -> Result<(), String> {
        self.input_proxy_counts.push(1);

        // Add corresponding output on the input proxy via NodeStates
        let mut ns = self
            .internal_graph
            .node_states()
            .write()
            .map_err(|e| e.to_string())?;
        ns.add_output_slot(&self.input_proxy_id, label, data_type);
        Ok(())
    }

    /// Add a single external input port that maps to multiple proxy outputs.
    ///
    /// Used for ports like "Baseline" where N edges connect to one input slot
    /// and each edge value is distributed to a separate proxy output inside
    /// the internal graph.
    pub fn add_multi_input(
        &mut self,
        proxy_count: usize,
        proxy_data_type: DataType,
        proxy_label: &'static str,
    ) -> Result<(), String> {
        self.input_proxy_counts.push(proxy_count);

        // Add proxy_count output slots on the input proxy via NodeStates
        let mut ns = self
            .internal_graph
            .node_states()
            .write()
            .map_err(|e| e.to_string())?;
        for _ in 0..proxy_count {
            ns.add_output_slot(&self.input_proxy_id, proxy_label, proxy_data_type);
        }
        Ok(())
    }

    /// Add a dynamic multi-input slot (phase 1: reserve the slot).
    ///
    /// Unlike `add_multi_input` (fixed proxy count), a dynamic input starts
    /// with zero proxy outputs. Call [`set_dynamic_input_target`] afterwards
    /// to register the internal target node (phase 2).
    pub fn add_dynamic_input_slot(&mut self) {
        self.input_proxy_counts.push(0);
        self.dynamic_input_targets.push(None);
    }

    /// Register the internal target for a dynamic multi-input slot (phase 2).
    ///
    /// Must be called after `add_dynamic_input_slot` and after internal wiring
    /// has created the target node.
    pub fn set_dynamic_input_target(
        &mut self,
        slot_index: usize,
        target_node_id: NodeId,
        target_slot: usize,
        proxy_data_type: DataType,
        proxy_label: &'static str,
    ) {
        // Ensure dynamic_input_targets is long enough
        while self.dynamic_input_targets.len() <= slot_index {
            self.dynamic_input_targets.push(None);
        }
        self.dynamic_input_targets[slot_index] = Some(DynamicInputTarget {
            internal_node_id: target_node_id,
            internal_slot: target_slot,
            proxy_data_type,
            proxy_label,
        });
    }

    /// Combined add + target setup for use outside `build_bim_subgraph`.
    pub fn add_dynamic_input(
        &mut self,
        target_node_id: NodeId,
        target_slot: usize,
        proxy_data_type: DataType,
        proxy_label: &'static str,
    ) {
        self.add_dynamic_input_slot();
        let slot_index = self.input_proxy_counts.len() - 1;
        self.set_dynamic_input_target(
            slot_index,
            target_node_id,
            target_slot,
            proxy_data_type,
            proxy_label,
        );
    }

    /// Synchronise dynamic input proxy outputs with external edge counts.
    ///
    /// For each dynamic input slot, counts the actual number of connected
    /// edges and grows/shrinks the internal proxy outputs and edges to match.
    ///
    /// Uses `insert_output_slot_at` to insert proxy outputs at the correct
    /// position (proxy_start) so that existing edges to higher-indexed slots
    /// remain valid after index shifting.
    ///
    /// Must be called before `inject_inputs`.
    fn sync_dynamic_inputs(
        &mut self,
        parent_node_id: &NodeId,
        parent_node_states: &SharedNodeStates,
    ) -> Result<(), String> {
        // Fast path: skip entirely when no dynamic inputs are registered.
        if self.dynamic_input_targets.iter().all(Option::is_none) {
            return Ok(());
        }

        let parent_ns = parent_node_states.read().map_err(|e| e.to_string())?;

        for (slot_idx, target) in self.dynamic_input_targets.iter().enumerate() {
            let Some(target) = target else { continue };
            let current_count = self.input_proxy_counts.get(slot_idx).copied().unwrap_or(0);

            // Count edges connected to this external input slot
            let edge_count = parent_ns
                .input_slot(parent_node_id, slot_idx)
                .map(|slot_state| parent_ns.edges_for_input(&slot_state.id).len())
                .unwrap_or(0);

            if edge_count == current_count {
                continue;
            }

            // Calculate the proxy output start index for this slot
            let proxy_start: usize = self.input_proxy_counts[..slot_idx].iter().sum();

            let mut internal_ns = self
                .internal_graph
                .node_states()
                .write()
                .map_err(|e| e.to_string())?;

            if edge_count > current_count {
                // Grow: insert proxy outputs at the correct position and create internal edges.
                // Inserting shifts existing higher-indexed slots up, keeping their edges valid
                // because edges store slot IDs (not indices).
                for i in current_count..edge_count {
                    let insert_idx = proxy_start + i;
                    let new_slot_id = internal_ns.insert_output_slot_at(
                        &self.input_proxy_id,
                        insert_idx,
                        target.proxy_label,
                        target.proxy_data_type,
                    );
                    // Create internal edge: new proxy output → target node's multi-input slot
                    if let Some(to_slot) = internal_ns
                        .input_slot(&target.internal_node_id, target.internal_slot)
                    {
                        let edge_id = EdgeId::new();
                        let edge = Edge {
                            from_node_id: self.input_proxy_id,
                            from_output_slot_index: insert_idx,
                            from_output_slot_id: new_slot_id,
                            to_node_id: target.internal_node_id,
                            to_input_slot_index: target.internal_slot,
                            to_input_slot_id: to_slot.id,
                        };
                        internal_ns.add_edge(edge_id, edge);
                    }
                }
            } else {
                // Shrink: remove excess proxy outputs and their internal edges
                for _ in edge_count..current_count {
                    let remove_idx = proxy_start + edge_count;
                    // Remove internal edges from this proxy output
                    if let Some(out_slot) =
                        internal_ns.output_slot(&self.input_proxy_id, remove_idx)
                    {
                        let slot_id = out_slot.id;
                        let outgoing = internal_ns
                            .outgoing_edges_for_node(&self.input_proxy_id)
                            .to_vec();
                        for eid in outgoing {
                            if internal_ns
                                .get_edge(&eid)
                                .is_some_and(|e| e.from_output_slot_id == slot_id)
                            {
                                internal_ns.remove_edge(&eid);
                            }
                        }
                    }
                    internal_ns.remove_output_slot(&self.input_proxy_id, remove_idx);
                }
            }

            drop(internal_ns);
            self.input_proxy_counts[slot_idx] = edge_count;
        }

        Ok(())
    }

    /// Remove an input slot from this subgraph node and the
    /// corresponding output slot(s) from the internal SubGraphInputNode.
    ///
    /// Returns an error if there are no inputs to remove.
    pub fn remove_input(&mut self, index: usize) -> Result<(), String> {
        let input_count = self.input_proxy_counts.len();
        if index >= input_count {
            return Err(format!(
                "Input index {} out of range (have {})",
                index, input_count
            ));
        }

        // Calculate the proxy output range for this input
        let proxy_start: usize = self.input_proxy_counts[..index].iter().sum();
        let proxy_count = self.input_proxy_counts[index];

        // Remove from internal NodeStates
        let mut ns = self
            .internal_graph
            .node_states()
            .write()
            .map_err(|e| e.to_string())?;
        for _ in 0..proxy_count {
            ns.remove_output_slot(&self.input_proxy_id, proxy_start);
        }
        drop(ns);

        self.input_proxy_counts.remove(index);

        Ok(())
    }

    /// Remove an output slot from this subgraph node and the
    /// corresponding input slot from the internal SubGraphOutputNode.
    ///
    /// Returns an error if the index is out of range.
    pub fn remove_output(&mut self, index: usize) -> Result<(), String> {
        let count = self.output_count();
        if index >= count {
            return Err(format!(
                "Output index {} out of range (have {})",
                index, count
            ));
        }

        // Remove from internal NodeStates
        let mut ns = self
            .internal_graph
            .node_states()
            .write()
            .map_err(|e| e.to_string())?;
        ns.remove_input_slot(&self.output_proxy_id, index);

        Ok(())
    }

    /// Add an output slot to this subgraph node and a corresponding
    /// input slot on the internal SubGraphOutputNode (via NodeStates).
    pub fn add_output(&mut self, label: &'static str, data_type: DataType) -> Result<(), String> {
        // Add corresponding input on the output proxy via NodeStates
        let mut ns = self
            .internal_graph
            .node_states()
            .write()
            .map_err(|e| e.to_string())?;
        ns.add_input_slot(&self.output_proxy_id, label, data_type, None);
        Ok(())
    }

    /// Inject external input data into the internal input proxy's output cache.
    ///
    /// Uses a cursor to map external inputs to proxy outputs:
    /// - For normal inputs (count=1): one value → one proxy output (with default fallback)
    /// - For multi-inputs (count>1): N edge values → N proxy outputs (no default)
    ///
    /// All slot metadata is read from NodeStates (parent and internal).
    fn inject_inputs(
        &self,
        parent_node_id: &NodeId,
        parent_cache: &SharedExecutionCache,
        parent_node_states: &SharedNodeStates,
    ) -> Result<(), String> {
        let internal_ns = self
            .internal_graph
            .node_states()
            .read()
            .map_err(|e| e.to_string())?;
        let internal_cache = self.internal_graph.shared_cache();

        let mut proxy_cursor: usize = 0;
        let input_count = {
            let ns = parent_node_states.read().map_err(|e| e.to_string())?;
            ns.input_slot_count(parent_node_id)
        };

        for idx in 0..input_count {
            let count = self.input_proxy_counts.get(idx).copied().unwrap_or(1);

            // Collect all edge values for this input slot
            // Read from parent NodeStates for edge topology, parent cache for output data
            let ns = parent_node_states.read().map_err(|e| e.to_string())?;
            let parent_cache_read = parent_cache.read()?;
            let mut data_values = Vec::new();
            let default_value_ref = if let Some(slot_state) = ns.input_slot(parent_node_id, idx) {
                for edge_id in ns.edges_for_input(&slot_state.id) {
                    if let Some(edge) = ns.get_edge(edge_id) {
                        if let Some(data) = parent_cache_read.outputs.get(&edge.from_output_slot_id)
                        {
                            data_values.push(data.share());
                        }
                    }
                }
                slot_state.default_value.as_ref().map(Arc::clone)
            } else {
                None
            };
            drop(parent_cache_read);
            drop(ns);

            if count == 1 {
                // Normal 1:1 mapping with default value fallback
                if let Some(data) = data_values.into_iter().next() {
                    if let Some(proxy_slot) =
                        internal_ns.output_slot(&self.input_proxy_id, proxy_cursor)
                    {
                        let mut cache = internal_cache.lock()?;
                        cache.outputs.insert(proxy_slot.id, data);
                    }
                } else if let Some(default_ref) = default_value_ref {
                    if let Ok(default_data) = Data::from_any(default_ref) {
                        if let Some(proxy_slot) =
                            internal_ns.output_slot(&self.input_proxy_id, proxy_cursor)
                        {
                            let mut cache = internal_cache.lock()?;
                            cache.outputs.insert(proxy_slot.id, default_data);
                        }
                    }
                }
            } else {
                // Multi-input: distribute N edge values to N proxy outputs
                let distribute_count = data_values.len().min(count);
                for (i, data) in data_values.iter().enumerate().take(distribute_count) {
                    if let Some(proxy_slot) =
                        internal_ns.output_slot(&self.input_proxy_id, proxy_cursor + i)
                    {
                        let mut cache = internal_cache.lock()?;
                        cache.outputs.insert(proxy_slot.id, data.share());
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
    pub(crate) async fn execute_internal(
        &mut self,
        parent_node_id: &NodeId,
        parent_cache: SharedExecutionCache,
        parent_node_states: SharedNodeStates,
    ) -> Result<(), String> {
        self.sync_dynamic_inputs(parent_node_id, &parent_node_states)?;
        self.inject_inputs(parent_node_id, &parent_cache, &parent_node_states)?;
        // Mark all internal nodes dirty so they execute.
        // This is needed because the internal graph doesn't know that
        // its proxy inputs changed.
        self.internal_graph.mark_all_nodes_dirty();
        let _changes = self.internal_graph.execute().await?;
        self.collect_outputs(parent_node_id, &parent_cache, &parent_node_states)?;
        Ok(())
    }

    /// Read results from the internal output proxy and write to this node's outputs.
    ///
    /// Edge topology is read from internal graph's NodeStates; output values
    /// from internal cache. Results are written to parent cache using output
    /// slot IDs from parent NodeStates.
    fn collect_outputs(
        &self,
        parent_node_id: &NodeId,
        parent_cache: &SharedExecutionCache,
        parent_node_states: &SharedNodeStates,
    ) -> Result<(), String> {
        let internal_ns = self
            .internal_graph
            .node_states()
            .read()
            .map_err(|e| e.to_string())?;
        let internal_cache = self.internal_graph.shared_cache();
        let parent_ns = parent_node_states.read().map_err(|e| e.to_string())?;
        let output_count = parent_ns.output_slot_count(parent_node_id);

        for idx in 0..output_count {
            let parent_output_slot_id = match parent_ns.output_slot(parent_node_id, idx) {
                Some(s) => s.id,
                None => continue,
            };

            // Find edges connected to the output proxy's input slot at this index
            if let Some(slot_state) = internal_ns.input_slot(&self.output_proxy_id, idx) {
                let cache_read = internal_cache.read()?;
                for edge_id in internal_ns.edges_for_input(&slot_state.id) {
                    if let Some(edge) = internal_ns.get_edge(edge_id) {
                        if let Some(data) = cache_read.outputs.get(&edge.from_output_slot_id) {
                            let data = data.share();
                            drop(cache_read);
                            // Write to parent cache output
                            let mut parent_write = parent_cache.lock()?;
                            parent_write.outputs.insert(parent_output_slot_id, data);
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

    fn register_in(manager: &mut NodeManager) -> Result<(), String> {
        let factory = Arc::new(|| {
            SubGraphNode::new("SubGraph")
                .map(|node| Arc::new(std::sync::RwLock::new(node)) as crate::NodeEntity)
        });
        manager.register_factory::<SubGraphNode>(factory.clone())?;
        manager.register_factory_with_name("SubGraph", factory, None);
        Ok(())
    }
}

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
    use crate::{NodeGraphRead, NodeGraphWrite};

    #[tokio::test]
    async fn test_subgraph_creation() {
        let mut graph = NodeGraph::new().expect("Failed to create graph");
        let subgraph_id = graph
            .create_node::<SubGraphNode>()
            .expect("Failed to create subgraph");

        let node = graph.get_node_by_id(&subgraph_id).expect("Node not found");
        let read = node.read().expect("Lock failed");
        assert_eq!(read.node_name(), "SubGraph");
        drop(read);

        let ns = graph.node_states().read().expect("NodeStates lock");
        assert_eq!(ns.input_slot_count(&subgraph_id), 0);
        assert_eq!(ns.output_slot_count(&subgraph_id), 0);
    }

    #[tokio::test]
    async fn test_subgraph_has_proxies() {
        let subgraph = SubGraphNode::new("Test").expect("Failed to create subgraph");
        assert!(subgraph
            .internal_graph
            .get_node_by_id(&subgraph.input_proxy_id)
            .is_some());
        assert!(subgraph
            .internal_graph
            .get_node_by_id(&subgraph.output_proxy_id)
            .is_some());
    }
}
