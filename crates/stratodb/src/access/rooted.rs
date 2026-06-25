//! A [`Reader`] re-rooted at an arbitrary node key.
//!
//! [`SData::load`](crate::data::SData::load) walks the tree by *path*, always
//! starting from the table root. An index query, by contrast, yields the stable
//! *keys* of the matching entities — not their paths (a path can shift, e.g. when
//! a preceding list element is removed; the key never does). [`Rooted`] bridges
//! the two: it wraps a reader and makes a node key look like the root, so a
//! relative path resolves from that node. Loading an entity is then just
//! `T::load(&Rooted::new(reader, entity_key), &SPath::root())`.
//!
//! Key-addressed reads (`scalar`, `child`, `kind`, `len`, `object_keys`) pass
//! straight through. Path-addressed reads (`resolve`, `scalar_at`) walk from the
//! re-rooted node. Resolution is never cached: the synthetic paths are relative
//! to the entity and would collide across entities in the shared path cache.

use super::Reader;
use crate::{
    data::Scalar,
    error::{SdbError, SdbResult},
    node::NodeKind,
    path::{SPath, Segment},
    Skey,
};

/// A view of an inner [`Reader`] whose root is the node `root`.
pub(crate) struct Rooted<'a, R> {
    inner: &'a R,
    root:  Skey,
}

impl<'a, R: Reader> Rooted<'a, R> {
    pub(crate) fn new(inner: &'a R, root: Skey) -> Self {
        Self {
            inner,
            root,
        }
    }
}

impl<R: Reader> Reader for Rooted<'_, R> {
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        let mut key = self.root;
        for seg in path.segments() {
            match self.inner.child(key, seg)? {
                Some(child) => key = child,
                None => return Ok(None),
            }
        }

        Ok(Some(key))
    }

    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        self.inner.child(parent, seg)
    }

    // Deliberately bypasses the path cache: see the module docs.
    fn child_cached(&self, parent: Skey, seg: &Segment, _child_path: &SPath) -> SdbResult<Option<Skey>> {
        self.inner.child(parent, seg)
    }

    fn scalar(&self, key: Skey) -> SdbResult<Scalar> {
        self.inner.scalar(key)
    }

    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let Some(key) = self.resolve(path)? else {
            return Ok(None);
        };

        match self.inner.kind(key)? {
            Some(NodeKind::Leaf) => Ok(Some(self.inner.scalar(key)?)),
            Some(other) => Err(SdbError::UnexpectedNode {
                path:     path.clone(),
                expected: "leaf",
                found:    other.as_str(),
            }),
            None => Err(SdbError::Corrupt("path resolves to a missing node".into())),
        }
    }

    fn kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        self.inner.kind(key)
    }

    fn len(&self, key: Skey) -> SdbResult<usize> {
        self.inner.len(key)
    }

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        self.inner.object_keys(key)
    }
}
