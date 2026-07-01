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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{access::ReadCursor, StratoDb};

    fn path(s: &str) -> SPath {
        SPath::parse(s).unwrap()
    }

    #[test]
    fn re_roots_reads_at_an_inner_node() {
        let db = StratoDb::create_in_memory().unwrap();
        let table = db.open_table("t").unwrap();

        let w = table.write().unwrap();
        w.put("users/alice/age", &30i32).unwrap();
        w.put("users/alice/tags[0]", &1i32).unwrap();
        w.put("users/alice/tags[1]", &2i32).unwrap();
        w.commit().unwrap();

        let txn = table.read().unwrap();
        let cursor = ReadCursor::new(&txn);
        let alice = cursor.resolve(&path("users/alice")).unwrap().unwrap();

        let rooted = Rooted::new(&cursor, alice);

        // Path reads resolve relative to `alice` (walking one and several segments).
        let age = rooted.resolve(&path("age")).unwrap().unwrap();
        assert!(rooted.resolve(&path("tags[1]")).unwrap().is_some());
        assert!(rooted.resolve(&path("missing")).unwrap().is_none());
        assert_eq!(rooted.scalar_at(&path("age")).unwrap(), Some(Scalar::I32(30)));

        // Key-addressed reads pass straight through to the inner reader.
        assert_eq!(rooted.scalar(age).unwrap(), Scalar::I32(30));
        assert_eq!(rooted.kind(age).unwrap(), Some(NodeKind::Leaf));
        assert!(rooted.child(alice, &Segment::Name("age".into())).unwrap().is_some());
        assert!(
            rooted
                .child_cached(alice, &Segment::Name("age".into()), &SPath::root())
                .unwrap()
                .is_some()
        );
        assert_eq!(
            rooted.object_keys(alice).unwrap(),
            vec![String::from("age"), String::from("tags")]
        );

        let tags = rooted.resolve(&path("tags")).unwrap().unwrap();
        assert_eq!(rooted.len(tags).unwrap(), 2);

        // A path landing on a non-leaf (the re-rooted node itself) is the wrong kind.
        assert!(matches!(
            rooted.scalar_at(&SPath::root()),
            Err(SdbError::UnexpectedNode { .. })
        ));
    }
}
