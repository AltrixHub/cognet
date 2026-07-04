//! Execution context passed to nodes during graph execution.
//!
//! `ExecutionContext` encapsulates all data a node needs to execute:
//! pre-resolved input values, node data, and an output writer.
//! This decouples nodes from the execution cache, making them pure
//! computation units.

use crate::{Data, DataType, NodePath, OutputSlotId, SharedExecutionCache};

/// Context provided to `NodeImpl::execute()`.
///
/// Contains pre-resolved input values and an output writer,
/// eliminating the need for nodes to interact with the cache directly.
pub struct ExecutionContext {
    /// The executing node's own hierarchical identity. Stable for the
    /// node's lifetime (the `NodeId` segments are minted once at node
    /// creation), so callers may derive persistent per-node identifiers
    /// from its rendering.
    pub path: NodePath,
    /// The node's own data (e.g., a Number node's stored value).
    pub node_data: Option<Data>,
    /// Pre-resolved input values for each input slot.
    /// `input_values[slot_index]` contains all values connected to that slot.
    pub input_values: Vec<Vec<Data>>,
    /// Writer for setting output slot values.
    pub output_writer: OutputWriter,
}

/// Writes output data to the execution cache during node execution.
///
/// Each output slot has a known `OutputSlotId` and expected `DataType`.
/// Type validation is performed on each write.
pub struct OutputWriter {
    cache: SharedExecutionCache,
    slots: Vec<(OutputSlotId, DataType)>,
}

impl OutputWriter {
    /// Create a new OutputWriter.
    pub fn new(cache: SharedExecutionCache, slots: Vec<(OutputSlotId, DataType)>) -> Self {
        Self { cache, slots }
    }

    /// Set the output value for a given slot index.
    ///
    /// Validates that the data type matches the slot's expected type,
    /// then writes the value to the execution cache.
    pub fn set(&self, slot_index: usize, data: Data) -> Result<(), String> {
        let (slot_id, expected_type) = self
            .slots
            .get(slot_index)
            .ok_or_else(|| format!("Output slot index out of range: {}", slot_index))?;

        if expected_type != &data.get_type() {
            return Err(format!(
                "Type mismatch on output slot {}: expected {:?}, got {:?}",
                slot_index,
                expected_type,
                data.get_type()
            ));
        }

        self.cache.lock()?.outputs.insert(*slot_id, data);
        Ok(())
    }
}
