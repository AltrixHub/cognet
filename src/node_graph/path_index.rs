//! Direct-children index for hierarchical NodeStates.
//!
//! Maps a parent NodePath → ordered list of NodeIds of its direct
//! children. Used by NodeStates to enumerate "children of a subgraph"
//! without scanning every path entry.

use std::collections::HashMap;

use crate::{NodeId, NodePath};

// PathIndex is wired into NodeStates in the next phase (P3b).
// Until then, the struct and its methods have no non-test caller,
// which is expected by design.
#[allow(dead_code)]
#[derive(Debug, Default, Clone)]
pub(crate) struct PathIndex {
    by_parent: HashMap<NodePath, Vec<NodeId>>,
}

#[allow(dead_code)]
impl PathIndex {
    pub fn insert(&mut self, path: &NodePath) {
        let Some(local) = path.leaf() else {
            return;
        };
        let parent = path.parent().unwrap_or_else(NodePath::root);
        let entry = self.by_parent.entry(parent).or_default();
        if !entry.contains(&local) {
            entry.push(local);
        }
    }

    pub fn remove(&mut self, path: &NodePath) {
        let Some(local) = path.leaf() else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if let Some(v) = self.by_parent.get_mut(&parent) {
            v.retain(|id| *id != local);
            if v.is_empty() {
                self.by_parent.remove(&parent);
            }
        }
    }

    pub fn children_of(&self, parent: &NodePath) -> &[NodeId] {
        self.by_parent
            .get(parent)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeId;

    #[test]
    fn empty_index_returns_no_children() {
        let idx = PathIndex::default();
        assert!(idx.children_of(&NodePath::root()).is_empty());
    }

    #[test]
    fn insert_then_query_returns_child() {
        let mut idx = PathIndex::default();
        let id = NodeId::new();
        idx.insert(&NodePath::root().child(id));
        assert_eq!(idx.children_of(&NodePath::root()), &[id]);
    }

    #[test]
    fn remove_drops_child() {
        let mut idx = PathIndex::default();
        let id = NodeId::new();
        let p = NodePath::root().child(id);
        idx.insert(&p);
        idx.remove(&p);
        assert!(idx.children_of(&NodePath::root()).is_empty());
    }

    #[test]
    fn nested_paths_isolated_per_parent() {
        let mut idx = PathIndex::default();
        let a = NodeId::new();
        let b = NodeId::new();
        let p1 = NodePath::root().child(a);
        let p2 = p1.child(b);
        idx.insert(&p1);
        idx.insert(&p2);
        assert_eq!(idx.children_of(&NodePath::root()), &[a]);
        assert_eq!(idx.children_of(&p1), &[b]);
    }
}
