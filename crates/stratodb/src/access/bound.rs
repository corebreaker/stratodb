//! A read/write cursor bound to one node-store backend, anchored at a node key.
//!
//! A whole-value `store` decomposes into many per-field writes. Routed through the
//! per-call [`WriteCursor`](super::WriteCursor), each one reopens the engine table,
//! re-runs index maintenance and re-walks the path from the table root — costs
//! that scale with the number of fields. [`BoundCursor`] removes all three: the
//! caller holds the backend once, brackets index maintenance once around the whole
//! store, and resolves the entity's parent a single time; every field write then
//! reuses that backend and resolves **relative to the anchor**.
//!
//! It is generic over the backend ([`WriteNodes`]), so the same store/decompose
//! logic builds a subtree both on the live engine table and inside an in-memory
//! [`MemNodes`](crate::engine::MemNodes) that becomes a packed entity's blob.
//!
//! It is used only for the additive phase of a store, after the old subtree has
//! been cleared, so it never participates in the shared path cache and does no
//! index maintenance of its own.

use super::{Reader, Writer};
use crate::{
    data::Scalar,
    engine::WriteNodes,
    error::{SdbError, SdbResult},
    node::{Node, NodeKind},
    path::{SPath, Segment},
    tree,
    Skey,
};

use std::cell::RefCell;

/// A cursor over a borrowed node-store backend whose paths resolve relative to
/// `root`. The backend is shared through a `RefCell` so each (read or write)
/// method can borrow it for the length of one operation.
pub(crate) struct BoundCursor<'a, 'b, B: WriteNodes> {
    backend: &'a RefCell<&'b mut B>,
    root:    Skey,
}

impl<'a, 'b, B: WriteNodes> BoundCursor<'a, 'b, B> {
    pub(crate) fn new(backend: &'a RefCell<&'b mut B>, root: Skey) -> Self {
        Self {
            backend,
            root,
        }
    }
}

impl<B: WriteNodes> Reader for BoundCursor<'_, '_, B> {
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        tree::resolve_from(&**self.backend.borrow(), self.root, path)
    }

    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        tree::child_key(&**self.backend.borrow(), parent, seg)
    }

    // The bound cursor sees its own uncommitted writes, so — like the write
    // cursor — it never serves resolutions from the shared path cache.
    fn child_cached(&self, parent: Skey, seg: &Segment, _child_path: &SPath) -> SdbResult<Option<Skey>> {
        self.child(parent, seg)
    }

    fn scalar(&self, key: Skey) -> SdbResult<Scalar> {
        tree::scalar_at(&**self.backend.borrow(), key)
    }

    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let backend = self.backend.borrow();
        let Some(key) = tree::resolve_from(&**backend, self.root, path)? else {
            return Ok(None);
        };

        match tree::read_node(&**backend, key)? {
            Some(Node::Leaf(scalar)) => Ok(Some(scalar)),
            Some(other) => Err(SdbError::UnexpectedNode {
                path:     path.clone(),
                expected: "leaf",
                found:    other.kind().as_str(),
            }),
            None => Err(SdbError::Corrupt("path resolves to a missing node".into())),
        }
    }

    fn kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        tree::kind_of(&**self.backend.borrow(), key)
    }

    fn len(&self, key: Skey) -> SdbResult<usize> {
        tree::list_len(&**self.backend.borrow(), key)
    }

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        tree::object_keys(&**self.backend.borrow(), key)
    }
}

impl<B: WriteNodes> Writer for BoundCursor<'_, '_, B> {
    fn put_scalar(&self, path: &SPath, scalar: Scalar) -> SdbResult<()> {
        tree::put_scalar_rel(&mut **self.backend.borrow_mut(), self.root, path, scalar)
    }

    fn remove(&self, path: &SPath) -> SdbResult<bool> {
        tree::remove_rel(&mut **self.backend.borrow_mut(), self.root, path)
    }

    fn ensure_container(&self, path: &SPath, list: bool) -> SdbResult<Skey> {
        tree::ensure_container_rel(&mut **self.backend.borrow_mut(), self.root, path, list)
    }

    fn list_move(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
        tree::list_move(&mut **self.backend.borrow_mut(), list_key, from, to)
    }

    fn list_swap(&self, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
        tree::list_swap(&mut **self.backend.borrow_mut(), list_key, i, j)
    }

    fn clear_children(&self, _path: &SPath, key: Skey) -> SdbResult<()> {
        tree::clear_children(&mut **self.backend.borrow_mut(), key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MemNodes;

    fn path(s: &str) -> SPath {
        SPath::parse(s).unwrap()
    }

    #[test]
    fn reads_and_writes_relative_to_the_anchor() {
        let mut backend = MemNodes::new();
        let cell = RefCell::new(&mut backend);
        let cursor = BoundCursor::new(&cell, Skey::ROOT);

        // Build an object root holding a leaf and a two-element list.
        cursor.ensure_container(&SPath::root(), false).unwrap();
        cursor.put_scalar(&path("a"), Scalar::I32(1)).unwrap();
        cursor.ensure_container(&path("xs"), true).unwrap();
        cursor.put_scalar(&path("xs[0]"), Scalar::I32(10)).unwrap();
        cursor.put_scalar(&path("xs[1]"), Scalar::I32(20)).unwrap();

        // Path- and key-addressed reads, all relative to the anchor.
        assert_eq!(cursor.scalar_at(&path("a")).unwrap(), Some(Scalar::I32(1)));
        assert!(cursor.scalar_at(&path("missing")).unwrap().is_none());

        let root = cursor.resolve(&SPath::root()).unwrap().unwrap();
        assert_eq!(cursor.kind(root).unwrap(), Some(NodeKind::Object));
        assert_eq!(
            cursor.object_keys(root).unwrap(),
            vec![String::from("a"), String::from("xs")]
        );

        let a = cursor.child(root, &Segment::Name("a".into())).unwrap().unwrap();
        assert_eq!(cursor.scalar(a).unwrap(), Scalar::I32(1));
        assert_eq!(
            cursor
                .child_cached(root, &Segment::Name("a".into()), &path("a"))
                .unwrap(),
            Some(a)
        );

        let xs = cursor.resolve(&path("xs")).unwrap().unwrap();
        assert_eq!(cursor.len(xs).unwrap(), 2);

        // Reorder, remove, clear.
        cursor.list_swap(xs, 0, 1).unwrap();
        cursor.list_move(xs, 1, 0).unwrap();
        assert!(cursor.remove(&path("a")).unwrap());
        cursor.clear_children(&SPath::root(), xs).unwrap();
        assert_eq!(cursor.len(xs).unwrap(), 0);
    }

    #[test]
    fn scalar_at_on_a_non_leaf_reports_the_wrong_kind() {
        let mut backend = MemNodes::new();
        let cell = RefCell::new(&mut backend);
        let cursor = BoundCursor::new(&cell, Skey::ROOT);

        cursor.ensure_container(&SPath::root(), false).unwrap();
        cursor.ensure_container(&path("obj"), false).unwrap();

        assert!(matches!(
            cursor.scalar_at(&path("obj")),
            Err(SdbError::UnexpectedNode { .. })
        ));
    }
}
