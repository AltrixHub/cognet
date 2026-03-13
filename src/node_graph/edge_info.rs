//! Edge information type for convenient edge data access.

use crate::{EdgeId, NodeId};

/// Edge information for UI rendering and querying.
///
/// Provides a convenient snapshot of edge data without needing to
/// access the internal storage directly.
#[derive(Debug, Clone)]
pub struct EdgeInfo {
    /// Edge ID.
    pub id: EdgeId,
    /// Source node ID.
    pub from_node: NodeId,
    /// Source output slot index.
    pub from_output: usize,
    /// Target node ID.
    pub to_node: NodeId,
    /// Target input slot index.
    pub to_input: usize,
}
