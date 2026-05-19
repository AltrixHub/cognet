//! Structured error types for the graph engine.

use crate::{DataType, EdgeId, NodeId, NodePath};
use std::fmt;

/// Target of an error in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ErrorTarget {
    /// Error on a node.
    Node(NodeId),
    /// Error on an input port.
    InputPort { node_id: NodeId, slot_index: usize },
    /// Error on an output port.
    OutputPort { node_id: NodeId, slot_index: usize },
    /// Error on an edge.
    Edge(EdgeId),
    /// Error on the graph itself (no specific target).
    Graph,
}

/// Kind of graph error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Type mismatch between ports.
    TypeMismatch {
        from_type: DataType,
        to_type: DataType,
    },
    /// Connection limit exceeded on input port.
    ConnectionLimitExceeded { max: usize },
    /// Node not found.
    NodeNotFound,
    /// Slot not found.
    SlotNotFound,
    /// Cycle detected in graph.
    CycleDetected,
    /// Execution error.
    Execution(String),
    /// Other error.
    Other(String),
}

/// Structured graph error.
#[derive(Debug, Clone)]
pub struct GraphError {
    /// The target of the error.
    pub target: ErrorTarget,
    /// The kind of error.
    pub kind: ErrorKind,
}

impl GraphError {
    /// Create a new graph error.
    pub fn new(target: ErrorTarget, kind: ErrorKind) -> Self {
        Self { target, kind }
    }

    /// Create a type mismatch error.
    pub fn type_mismatch(
        node_id: NodeId,
        slot_index: usize,
        from_type: DataType,
        to_type: DataType,
    ) -> Self {
        Self {
            target: ErrorTarget::InputPort {
                node_id,
                slot_index,
            },
            kind: ErrorKind::TypeMismatch { from_type, to_type },
        }
    }

    /// Create a connection limit exceeded error.
    pub fn connection_limit_exceeded(node_id: NodeId, slot_index: usize, max: usize) -> Self {
        Self {
            target: ErrorTarget::InputPort {
                node_id,
                slot_index,
            },
            kind: ErrorKind::ConnectionLimitExceeded { max },
        }
    }

    /// Create a node not found error.
    pub fn node_not_found(node_id: NodeId) -> Self {
        Self {
            target: ErrorTarget::Node(node_id),
            kind: ErrorKind::NodeNotFound,
        }
    }

    /// Create a slot not found error for input.
    pub fn input_slot_not_found(node_id: NodeId, slot_index: usize) -> Self {
        Self {
            target: ErrorTarget::InputPort {
                node_id,
                slot_index,
            },
            kind: ErrorKind::SlotNotFound,
        }
    }

    /// Create a slot not found error for output.
    pub fn output_slot_not_found(node_id: NodeId, slot_index: usize) -> Self {
        Self {
            target: ErrorTarget::OutputPort {
                node_id,
                slot_index,
            },
            kind: ErrorKind::SlotNotFound,
        }
    }

    /// Create an execution error.
    pub fn execution(node_id: NodeId, message: impl Into<String>) -> Self {
        Self {
            target: ErrorTarget::Node(node_id),
            kind: ErrorKind::Execution(message.into()),
        }
    }

    /// Create an execution error addressed by a `NodePath`.
    ///
    /// This is the path-aware counterpart of [`Self::execution`]. It exists
    /// so executor/planner call sites can record per-node failures without
    /// risking a phantom `NodeId` when the path happens to be the root.
    ///
    /// `NodeId: Default` returns a freshly minted ULID, so a naive
    /// `path.leaf().unwrap_or_default()` at the call site would silently
    /// attach the error to a node that exists in no graph. Routing through
    /// this constructor keeps that footgun out of the executor: when
    /// `path.leaf()` is `None` we drop the per-node target, emit a warning
    /// to the `graph` tracing target so the regression is observable, and
    /// fall back to a graph-level `ErrorTarget::Graph` so downstream
    /// consumers still see the message.
    ///
    /// `ErrorTarget` still keys per-node errors by `NodeId` rather than
    /// `NodePath`; the remaining `path.leaf()` shim in `collect_outputs`
    /// is tracked by `// plan-007: ErrorTarget migration TBD`.
    pub fn execution_at_path(path: &NodePath, message: impl Into<String>) -> Self {
        let kind = ErrorKind::Execution(message.into());
        match path.leaf() {
            Some(node_id) => Self {
                target: ErrorTarget::Node(node_id),
                kind,
            },
            None => {
                tracing::warn!(
                    target: "graph",
                    path = %path,
                    "[cognet] execution error recorded against root path; \
                     attaching to ErrorTarget::Graph",
                );
                // plan-007: ErrorTarget migration TBD — once ErrorTarget
                // carries a path-aware variant we can preserve full context.
                Self {
                    target: ErrorTarget::Graph,
                    kind,
                }
            }
        }
    }

    /// Create a cycle detected error.
    pub fn cycle_detected() -> Self {
        Self {
            target: ErrorTarget::Graph,
            kind: ErrorKind::CycleDetected,
        }
    }

    /// Get the display message for this error.
    pub fn message(&self) -> String {
        match &self.kind {
            ErrorKind::TypeMismatch { from_type, to_type } => {
                format!(
                    "Type mismatch: cannot connect {:?} to {:?}",
                    from_type, to_type
                )
            }
            ErrorKind::ConnectionLimitExceeded { max } => {
                format!("Connection limit exceeded: maximum {} connection(s)", max)
            }
            ErrorKind::NodeNotFound => "Node not found".to_string(),
            ErrorKind::SlotNotFound => "Slot not found".to_string(),
            ErrorKind::CycleDetected => "Cycle detected in graph".to_string(),
            ErrorKind::Execution(msg) => format!("Execution error: {}", msg),
            ErrorKind::Other(msg) => msg.clone(),
        }
    }

    /// Check if this is a connection-related error (for UI highlighting).
    pub fn is_connection_error(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::TypeMismatch { .. } | ErrorKind::ConnectionLimitExceeded { .. }
        )
    }

    /// Check if this is an execution error.
    pub fn is_execution_error(&self) -> bool {
        matches!(self.kind, ErrorKind::Execution(_))
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for GraphError {}
