//! A read/write cursor bound to one already-open engine table and anchored at a
//! node key.
//!
//! A whole-value `store` decomposes into many per-field writes. Routed through the
//! per-call [`WriteCursor`](super::WriteCursor), each one reopens the engine table,
//! re-runs index maintenance and re-walks the path from the table root — costs
//! that scale with the number of fields. [`BoundCursor`] removes all three: the
//! caller opens the table once, brackets index maintenance once around the whole
//! store, and resolves the entity's parent a single time; every field write then
//! reuses that table handle and resolves **relative to the anchor** instead of the
//! root.
//!
//! It is used only for the additive phase of [`store`](crate::txn::WriteTxn::store)
//! / [`store_value`](crate::txn::WriteTxn::store_value), after the old subtree has
//! been cleared, so it never participates in the shared path cache and does no
//! index maintenance of its own.

use super::{Reader, Writer};
use crate::{
    data::Scalar,
    engine::{TableKey, TableValue},
    error::{SdbError, SdbResult},
    node::{Node, NodeKind},
    path::{SPath, Segment},
    tree,
    Skey,
};

use redb::Table;
use std::cell::RefCell;

/// The writable engine table over StratoDB keys and values.
type DataTable<'txn> = Table<'txn, TableKey, TableValue>;

/// A cursor over a borrowed, already-open table whose paths resolve relative to
/// `root`. The table is shared through a `RefCell` so each (read or write) method
/// can borrow it for the length of one engine operation.
pub(crate) struct BoundCursor<'a, 'b, 'txn> {
    table: &'a RefCell<&'b mut DataTable<'txn>>,
    root:  Skey,
}

impl<'a, 'b, 'txn> BoundCursor<'a, 'b, 'txn> {
    pub(crate) fn new(table: &'a RefCell<&'b mut DataTable<'txn>>, root: Skey) -> Self {
        Self {
            table,
            root,
        }
    }
}

impl Reader for BoundCursor<'_, '_, '_> {
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        tree::resolve_from(&**self.table.borrow(), self.root, path)
    }

    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        tree::child_key(&**self.table.borrow(), parent, seg)
    }

    // The bound cursor sees its own uncommitted writes, so — like the write
    // cursor — it never serves resolutions from the shared path cache.
    fn child_cached(&self, parent: Skey, seg: &Segment, _child_path: &SPath) -> SdbResult<Option<Skey>> {
        self.child(parent, seg)
    }

    fn scalar(&self, key: Skey) -> SdbResult<Scalar> {
        tree::scalar_at(&**self.table.borrow(), key)
    }

    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let table = self.table.borrow();
        let Some(key) = tree::resolve_from(&**table, self.root, path)? else {
            return Ok(None);
        };

        match tree::read_node(&**table, key)? {
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
        tree::kind_of(&**self.table.borrow(), key)
    }

    fn len(&self, key: Skey) -> SdbResult<usize> {
        tree::list_len(&**self.table.borrow(), key)
    }

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        tree::object_keys(&**self.table.borrow(), key)
    }
}

impl Writer for BoundCursor<'_, '_, '_> {
    fn put_scalar(&self, path: &SPath, scalar: Scalar) -> SdbResult<()> {
        tree::put_scalar_rel(&mut self.table.borrow_mut(), self.root, path, scalar)
    }

    fn remove(&self, path: &SPath) -> SdbResult<bool> {
        tree::remove_rel(&mut self.table.borrow_mut(), self.root, path)
    }

    fn ensure_container(&self, path: &SPath, list: bool) -> SdbResult<Skey> {
        tree::ensure_container_rel(&mut self.table.borrow_mut(), self.root, path, list)
    }

    fn list_move(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
        tree::list_move(&mut self.table.borrow_mut(), list_key, from, to)
    }

    fn list_swap(&self, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
        tree::list_swap(&mut self.table.borrow_mut(), list_key, i, j)
    }

    fn clear_children(&self, _path: &SPath, key: Skey) -> SdbResult<()> {
        tree::clear_children(&mut self.table.borrow_mut(), key)
    }
}
