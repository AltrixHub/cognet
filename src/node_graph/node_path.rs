//! NodePath: hierarchical node identity.
//!
//! A `NodePath` is the runtime identity of a node in cognet's
//! hierarchy. `NodePath::root()` is the path of the root NodeGraph;
//! `path.child(local_id)` extends the path by descending into a
//! SubGraphNode's namespace.

use std::sync::Arc;

use crate::{EntityId, NodeId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodePath {
    segments: Arc<[NodeId]>,
}

impl NodePath {
    pub fn root() -> Self {
        Self {
            segments: Arc::from([] as [NodeId; 0]),
        }
    }

    pub fn child(&self, id: NodeId) -> Self {
        let mut v = Vec::with_capacity(self.segments.len() + 1);
        v.extend(self.segments.iter().copied());
        v.push(id);
        Self {
            segments: Arc::from(v),
        }
    }

    pub fn parent(&self) -> Option<Self> {
        if self.segments.is_empty() {
            return None;
        }
        let new_len = self.segments.len() - 1;
        let v: Vec<NodeId> = self.segments[..new_len].to_vec();
        Some(Self {
            segments: Arc::from(v),
        })
    }

    pub fn leaf(&self) -> Option<NodeId> {
        self.segments.last().copied()
    }

    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn segments(&self) -> &[NodeId] {
        &self.segments
    }
}

impl std::fmt::Display for NodePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_root() {
            return f.write_str("/");
        }
        for s in self.segments.iter() {
            f.write_str("/")?;
            f.write_str(&s.id_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeId;

    #[test]
    fn root_is_root_and_empty() {
        let r = NodePath::root();
        assert!(r.is_root());
        assert_eq!(r.segments().len(), 0);
        assert_eq!(r.leaf(), None);
        assert_eq!(r.parent(), None);
    }

    #[test]
    fn child_extends_and_parent_pops() {
        let a = NodeId::new();
        let b = NodeId::new();
        let p = NodePath::root().child(a).child(b);
        assert_eq!(p.segments().len(), 2);
        assert_eq!(p.leaf(), Some(b));
        assert_eq!(p.parent().and_then(|p| p.leaf()), Some(a));
        assert_eq!(
            p.parent().and_then(|p| p.parent()).map(|p| p.is_root()),
            Some(true)
        );
    }

    #[test]
    fn clone_is_cheap_and_equal() {
        let p = NodePath::root().child(NodeId::new());
        let q = p.clone();
        assert_eq!(p, q);
    }

    #[test]
    fn display_uses_slash_separator() {
        let root = NodePath::root();
        assert_eq!(format!("{}", root), "/");
    }
}
