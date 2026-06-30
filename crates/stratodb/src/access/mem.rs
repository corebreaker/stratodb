//! A read cursor over a packed entity.
//!
//! A packed entity is one engine value holding a whole subtree as an rkyv archive
//! ([`ArchivedNodes`]). To read it — a `load`, a `fetch` accessor, an index
//! column, an exported `Value` — the archive is navigated zero-copy with the very
//! same tree logic used on the live table, relative to a `root` key inside the
//! blob. `ArchivedNodes` is immutable here, so the reader is `Send + Sync` and can
//! back an `Arc<dyn Reader>` accessor.

use super::Reader;
use crate::{
    data::Scalar,
    engine::ArchivedNodes,
    error::{SdbError, SdbResult},
    node::{Node, NodeKind},
    path::{SPath, Segment},
    tree,
    Skey,
};

use std::sync::Arc;

/// A reader over one decoded packed entity. `root` is the node the enclosing path
/// resolved to inside the blob; `base` is that enclosing path, so the absolute
/// paths an accessor builds (e.g. `xs[0]`) can be re-based onto `root`. The blob
/// is held behind an `Arc` so it can be shared with (and served from) the blob
/// cache without cloning the decoded table.
pub(crate) struct MemReader {
    nodes: Arc<ArchivedNodes>,
    root:  Skey,
    base:  SPath,
}

impl MemReader {
    pub(crate) fn new(nodes: Arc<ArchivedNodes>, root: Skey, base: SPath) -> Self {
        Self {
            nodes,
            root,
            base,
        }
    }

    /// Resolves `path` within the blob. A path is taken relative to `root` after
    /// stripping the accessor `base` prefix (accessors address by absolute path);
    /// a path that does not start with `base` cannot resolve here.
    fn resolve_rebased(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        let base = self.base.segments();
        let segs = path.segments();
        if segs.len() < base.len() || segs[..base.len()] != *base {
            return Ok(None);
        }

        tree::resolve_from(&*self.nodes, self.root, &SPath::from_segments(&segs[base.len()..]))
    }
}

impl Reader for MemReader {
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        self.resolve_rebased(path)
    }

    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        tree::child_key(&*self.nodes, parent, seg)
    }

    fn scalar(&self, key: Skey) -> SdbResult<Scalar> {
        tree::scalar_at(&*self.nodes, key)
    }

    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let Some(key) = self.resolve_rebased(path)? else {
            return Ok(None);
        };

        match tree::read_node(&*self.nodes, key)? {
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
        tree::kind_of(&*self.nodes, key)
    }

    fn len(&self, key: Skey) -> SdbResult<usize> {
        tree::list_len(&*self.nodes, key)
    }

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        tree::object_keys(&*self.nodes, key)
    }
}
