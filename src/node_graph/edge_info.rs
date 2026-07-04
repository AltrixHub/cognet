//! Edge information type for convenient edge data access.

use crate::{EdgeId, NodePath};

/// Edge information for UI rendering and querying.
///
/// Provides a convenient snapshot of edge data without needing to
/// access the internal storage directly.
///
/// Both endpoints carry a full `NodePath` matching `Edge.from_node` /
/// `Edge.to_node` (plan-006 C17/C20).
#[derive(Debug, Clone)]
pub struct EdgeInfo {
    /// Edge ID.
    pub id: EdgeId,
    /// Source node path.
    pub from_node: NodePath,
    /// Source output slot index.
    pub from_output: usize,
    /// Target node path.
    pub to_node: NodePath,
    /// Target input slot index.
    pub to_input: usize,
}
