use super::NodeKind;
use crate::{data::Scalar, Skey};

// The on-disk node discriminant. A node's byte (de)serialization lives at the
// storage boundary (`engine::table_value`, which decodes a packed blob by
// reference rather than copying it); these constants are its single source of
// truth, kept beside the `Node` type they tag.
pub(crate) mod tag {
    pub(crate) const OBJECT: u8 = 0;
    pub(crate) const LIST: u8 = 1;
    pub(crate) const LEAF: u8 = 2;
    pub(crate) const PACKED: u8 = 3;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_maps_each_variant_and_reads_a_packed_root() {
        assert_eq!(Node::Object.kind(), NodeKind::Object);
        assert_eq!(Node::List(vec![]).kind(), NodeKind::List);
        assert_eq!(Node::Leaf(Scalar::Null).kind(), NodeKind::Leaf);
        assert_eq!(
            Node::Packed {
                root: NodeKind::List,
                blob: vec![],
            }
            .kind(),
            NodeKind::List
        );
    }
}
