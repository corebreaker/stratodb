use super::NodeKind;
use crate::{
    codec::{self, Reader},
    error::{SdbError, SdbResult},
    data::Scalar,
    Skey,
};

mod tag {
    pub(super) const OBJECT: u8 = 0;
    pub(super) const LIST: u8 = 1;
    pub(super) const LEAF: u8 = 2;
    pub(super) const PACKED: u8 = 3;
}

/// A stored node: either a container (object/list), a leaf, or a packed entity.
///
/// An object node carries no inline child map: its `(name -> child key)` links
/// are stored as separate [`TableKey::Child`](crate::engine::TableKey) entries so
/// that attaching, detaching and looking up a single child cost one engine
/// operation instead of rewriting the whole map (which made a wide object O(N) per
/// child write, hence O(N²) to fill). A list still holds its element keys inline:
/// element order is positional and lists are not the wide-fan-out case.
///
/// A `Packed` node holds a whole entity subtree serialized into one engine value
/// (a mini node-table — see [`crate::engine::backend`]). One `store` writes it as
/// a single entry and one `load` reads it back, so whole-entity I/O is one engine
/// operation instead of one per shredded node. Its children are addressed *inside*
/// the blob; the table only sees the packed entity as a single keyed node.
#[derive(Clone, Debug)]
pub(crate) enum Node {
    /// An object marker. Its children live in separate child-link entries.
    Object,
    /// A list: a zero-based sequence of child keys.
    List(Vec<Skey>),
    /// A leaf: a single scalar value.
    Leaf(Scalar),
    /// A packed entity subtree: the root node's kind plus the serialized mini
    /// node-table holding the whole subtree.
    Packed { root: NodeKind, blob: Vec<u8> },
}

impl Node {
    pub(crate) fn kind(&self) -> NodeKind {
        match self {
            Node::Object => NodeKind::Object,
            Node::List(_) => NodeKind::List,
            Node::Leaf(_) => NodeKind::Leaf,
            // A packed entity reports the kind of its subtree root.
            Node::Packed {
                root, ..
            } => *root,
        }
    }

    pub(crate) fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Node::Object => {
                buf.push(tag::OBJECT);
            }
            Node::List(items) => {
                buf.push(tag::LIST);
                codec::put_u32(buf, items.len() as u32);

                for key in items {
                    buf.extend_from_slice(&key.into_bytes());
                }
            }
            Node::Leaf(scalar) => {
                buf.push(tag::LEAF);
                scalar.encode(buf);
            }
            Node::Packed {
                root,
                blob,
            } => {
                buf.push(tag::PACKED);
                buf.push(root.as_tag());
                codec::put_bytes(buf, blob);
            }
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> SdbResult<Node> {
        match r.u8()? {
            tag::OBJECT => Ok(Node::Object),
            tag::PACKED => {
                let root = NodeKind::from_tag(r.u8()?)?;
                let blob = r.bytes()?.to_vec();

                Ok(Node::Packed {
                    root,
                    blob,
                })
            }
            tag::LIST => {
                let count = r.u32()? as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(Skey::from_bytes(r.array()?));
                }

                Ok(Node::List(items))
            }
            tag::LEAF => Ok(Node::Leaf(Scalar::decode(r)?)),
            other => Err(SdbError::Corrupt(format!("unknown node tag {other}"))),
        }
    }
}
