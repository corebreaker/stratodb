//! The rkyv-archived form of a packed entity, navigated zero-copy on read.
//!
//! A packed entity's blob is an rkyv archive of [`ArchTree`]: the entity's nodes
//! keyed by their (real, store-assigned) [`Skey`], sorted by key so a lookup is a
//! binary search straight over the archived bytes — no decode into an owned
//! structure. Object children and list elements reference one another by key, the
//! same identities the shredded tree used, so a node's `key()` is stable and
//! unique exactly as before. Leaf scalars are kept in StratoDB's own byte codec
//! (not archived through rkyv), so the archive only holds `[u8; 16]` / `String` /
//! `Vec<u8>`.
//!
//! [`ArchivedNodes`] implements the [`ReadNodes`](super::ReadNodes) contract, so
//! the very same tree logic drives a packed entity read in place.

use super::{backend::NodeIter, ReadNodes, TableKey, TableValue};
use crate::{
    data::Scalar,
    error::{SdbError, SdbResult},
    node::Node,
    Skey,
};

use rkyv::{rancor, util::AlignedVec};

/// One node in the archived tree, children referenced by 16-byte key.
#[derive(rkyv::Archive, rkyv::Serialize)]
pub(crate) enum ArchNode {
    /// An object: `(field name, child key)` pairs, sorted by name.
    Object(Vec<(String, [u8; 16])>),
    /// A list: child keys in order.
    List(Vec<[u8; 16]>),
    /// A leaf: the scalar in StratoDB's byte codec.
    Leaf(Vec<u8>),
}

/// A whole packed entity: `(key, node)` pairs, sorted by key.
#[derive(rkyv::Archive, rkyv::Serialize)]
pub(crate) struct ArchTree {
    pub(crate) nodes: Vec<([u8; 16], ArchNode)>,
}

/// Serializes `tree` into a packed-entity blob.
pub(crate) fn to_bytes(tree: &ArchTree) -> SdbResult<Vec<u8>> {
    let bytes = rkyv::to_bytes::<rancor::Error>(tree)
        .map_err(|err| SdbError::Corrupt(format!("rkyv serialize failed: {err}")))?;

    Ok(bytes.to_vec())
}

/// An owned snapshot of one archived node, with children as keys.
pub(crate) enum NodeView {
    Object(Vec<(String, Skey)>),
    List(Vec<Skey>),
    Leaf(Scalar),
}

/// A read-only, zero-copy view over an archived packed entity.
///
/// Holds the blob in an [`AlignedVec`] (rkyv requires alignment; bytes read from
/// the engine are not aligned) and re-accesses it per call. The archive is
/// validated once, at construction, so per-call access is unchecked and cheap.
pub(crate) struct ArchivedNodes {
    bytes: AlignedVec,
}

impl ArchivedNodes {
    /// Copies `blob` into an aligned buffer and validates it as an [`ArchTree`].
    pub(crate) fn new(blob: &[u8]) -> SdbResult<Self> {
        let mut bytes = AlignedVec::new();
        bytes.extend_from_slice(blob);

        rkyv::access::<ArchivedArchTree, rancor::Error>(&bytes)
            .map_err(|err| SdbError::Corrupt(format!("corrupt packed entity: {err}")))?;

        Ok(Self {
            bytes,
        })
    }

    /// Copies `blob` into an aligned buffer **without** validating it.
    ///
    /// The rkyv validation walks the whole archive (O(blob size)), which dominates
    /// a same-generation blob edit (see [`WriteTxn::put_scalar`](crate::txn::WriteTxn::put)).
    /// A write transaction only ever navigates a blob **it just read from its own
    /// table** — one StratoDB itself serialized — so the bytes are trusted and the
    /// scan is skipped. Do not use this on an externally supplied blob.
    pub(crate) fn new_unchecked(blob: &[u8]) -> Self {
        let mut bytes = AlignedVec::new();
        bytes.extend_from_slice(blob);

        Self {
            bytes,
        }
    }

    /// The archived tree. Safe: [`new`](Self::new) validated the bytes and they
    /// are immutable thereafter.
    fn tree(&self) -> &ArchivedArchTree {
        unsafe { rkyv::access_unchecked::<ArchivedArchTree>(&self.bytes) }
    }

    /// The archived node for `key`, found by binary search over the sorted pairs.
    fn node(&self, key: Skey) -> Option<&ArchivedArchNode> {
        let bytes = key.into_bytes();
        let nodes = &self.tree().nodes;

        nodes
            .binary_search_by(|pair| pair.0.cmp(&bytes))
            .ok()
            .map(|at| &nodes[at].1)
    }

    /// The `(offset, length)` of leaf `key`'s scalar bytes within the blob, or
    /// `None` if `key` is absent or is not a leaf. The offset is relative to the
    /// blob's start (the aligned copy is byte-identical to it), so a same-length
    /// scalar update can patch those bytes in place — no decode + re-serialize.
    pub(crate) fn leaf_byte_span(&self, key: Skey) -> Option<(usize, usize)> {
        match self.node(key)? {
            ArchivedArchNode::Leaf(bytes) => {
                let base = self.bytes.as_ptr() as usize;
                let at = bytes.as_slice().as_ptr() as usize;

                Some((at - base, bytes.len()))
            }
            _ => None,
        }
    }

    /// Every `(key, node view)` of the entity, materialized — used to rebuild a
    /// mutable [`MemNodes`](super::MemNodes) for in-place edits / unpacking.
    pub(crate) fn entries(&self) -> SdbResult<Vec<(Skey, NodeView)>> {
        self.tree()
            .nodes
            .iter()
            .map(|pair| Ok((Skey::from_bytes(pair.0), view_of(&pair.1)?)))
            .collect()
    }
}

/// Materializes an archived node into an owned [`NodeView`].
fn view_of(node: &ArchivedArchNode) -> SdbResult<NodeView> {
    let view = match node {
        ArchivedArchNode::Object(pairs) => NodeView::Object(
            pairs
                .iter()
                .map(|pair| (pair.0.as_str().to_string(), Skey::from_bytes(pair.1)))
                .collect(),
        ),
        ArchivedArchNode::List(items) => NodeView::List(items.iter().map(|key| Skey::from_bytes(*key)).collect()),
        ArchivedArchNode::Leaf(bytes) => NodeView::Leaf(Scalar::decode(&mut crate::codec::Reader::new(bytes))?),
    };

    Ok(view)
}

impl ReadNodes for ArchivedNodes {
    fn fetch(&self, key: &TableKey) -> SdbResult<Option<TableValue>> {
        match key {
            TableKey::Data(node_key) => {
                let Some(node) = self.node(*node_key) else {
                    return Ok(None);
                };

                let node = match node {
                    ArchivedArchNode::Object(_) => Node::Object,
                    ArchivedArchNode::List(items) => {
                        Node::List(items.iter().map(|key| Skey::from_bytes(*key)).collect())
                    }
                    ArchivedArchNode::Leaf(bytes) => Node::Leaf(Scalar::decode(&mut crate::codec::Reader::new(bytes))?),
                };

                Ok(Some(TableValue::Node(node)))
            }
            TableKey::Child {
                parent,
                name,
            } => Ok(self.child_link(*parent, name)?.map(TableValue::Skey)),
            TableKey::Index {
                ..
            } => Ok(None),
        }
    }

    fn scan_from(&self, lower: &TableKey) -> SdbResult<NodeIter<'_>> {
        let TableKey::Child {
            parent,
            name,
        } = lower
        else {
            return Ok(Box::new(std::iter::empty()));
        };

        let parent = *parent;
        let mut out = Vec::new();
        if let Some(ArchivedArchNode::Object(pairs)) = self.node(parent) {
            for pair in pairs.iter() {
                if pair.0.as_str() >= name.as_str() {
                    out.push(Ok((
                        TableKey::Child {
                            parent,
                            name: pair.0.as_str().to_string(),
                        },
                        TableValue::Skey(Skey::from_bytes(pair.1)),
                    )));
                }
            }
        }

        Ok(Box::new(out.into_iter()))
    }

    fn child_link(&self, parent: Skey, name: &str) -> SdbResult<Option<Skey>> {
        let Some(ArchivedArchNode::Object(pairs)) = self.node(parent) else {
            return Ok(None);
        };

        // Children are stored name-sorted, so a binary search finds the link.
        let found = pairs.binary_search_by(|pair| pair.0.as_str().cmp(name));

        Ok(found.ok().map(|at| Skey::from_bytes(pairs[at].1)))
    }
}
