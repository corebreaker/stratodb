//! The composite value type stored in a StratoDB data table.
//!
//! A [`TableValue`] is the payload an engine entry maps to: a node (for `Data`
//! keys), a referenced primary key (the entity, for unique index entries), or
//! nothing (for non-unique index entries, whose entity lives in the key).
//!
//! Reads decode into the **borrowed** [`TableValueRef`], whose `Packed` node keeps
//! the blob as a slice into the engine page rather than copying it: classifying a
//! node (is it a packed entity? — see [`ReadNodes::node_step`](super::ReadNodes))
//! then costs nothing per blob byte. A caller that needs an owned value calls
//! [`TableValueRef::into_owned`] (which copies the blob once, as before). Writes go
//! the other way — [`TableValue::as_ref`] borrows an owned value into a
//! `TableValueRef` for the engine to serialize, so a packed write copies the blob
//! exactly once (into the page), never twice.

use crate::{
    codec::{self, Reader},
    data::Scalar,
    error::{SdbError, SdbResult},
    key::Skey,
    node::{tag as node_tag, Node, NodeKind},
};

use redb::{TypeName, Value as RedbValue};
use std::borrow::Cow;

mod tag {
    pub(super) const NODE: u8 = 0;
    pub(super) const SKEY: u8 = 1;
    pub(super) const UNIT: u8 = 2;
}

/// The value of an entry in a StratoDB data table (owned form). Built by writers
/// and returned by [`ReadNodes::fetch`](super::ReadNodes); the engine (de)codes
/// through the borrowed [`TableValueRef`].
#[derive(Clone, Debug)]
pub(crate) enum TableValue {
    /// A stored node (for `Data` keys).
    Node(Node),
    /// A referenced primary key (the entity, for unique index entries).
    Skey(Skey),
    /// No payload (for non-unique index entries).
    Unit,
}

impl TableValue {
    /// Borrows this value as a [`TableValueRef`] for the engine to serialize — a
    /// packed node's blob is borrowed, not copied.
    pub(crate) fn as_ref(&self) -> TableValueRef<'_> {
        match self {
            TableValue::Node(node) => TableValueRef::Node(node_as_ref(node)),
            TableValue::Skey(skey) => TableValueRef::Skey(*skey),
            TableValue::Unit => TableValueRef::Unit,
        }
    }
}

/// The borrowed form the engine decodes into and serializes from. A `Packed`
/// node's blob is a slice into the source bytes (the engine page on read, the
/// owned node on write); a list's elements and a leaf's scalar are borrowed on
/// write and owned on read (`Cow`), so neither direction copies needlessly.
#[derive(Debug)]
pub(crate) enum TableValueRef<'a> {
    Node(NodeRef<'a>),
    Skey(Skey),
    Unit,
}

/// A node in borrowed form (see [`TableValueRef`]). The `Packed` blob is always a
/// borrow; the container payloads are `Cow` so a write borrows and a read owns.
#[derive(Debug)]
pub(crate) enum NodeRef<'a> {
    Object,
    List(Cow<'a, [Skey]>),
    Leaf(Cow<'a, Scalar>),
    Packed { root: NodeKind, blob: &'a [u8] },
}

/// Borrows an owned [`Node`] as a [`NodeRef`] without copying its payload.
fn node_as_ref(node: &Node) -> NodeRef<'_> {
    match node {
        Node::Object => NodeRef::Object,
        Node::List(items) => NodeRef::List(Cow::Borrowed(items)),
        Node::Leaf(scalar) => NodeRef::Leaf(Cow::Borrowed(scalar)),
        Node::Packed {
            root,
            blob,
        } => NodeRef::Packed {
            root: *root,
            blob,
        },
    }
}

impl<'a> TableValueRef<'a> {
    /// Decodes engine bytes, borrowing a packed blob straight from `data`.
    pub(crate) fn decode(data: &'a [u8]) -> SdbResult<TableValueRef<'a>> {
        let mut r = Reader::new(data);
        let value = match r.u8()? {
            tag::NODE => TableValueRef::Node(NodeRef::decode(&mut r)?),
            tag::SKEY => TableValueRef::Skey(Skey::from_bytes(r.array()?)),
            tag::UNIT => TableValueRef::Unit,
            other => return Err(SdbError::Corrupt(format!("unknown table value tag {other}"))),
        };

        Ok(value)
    }

    /// Serializes this value to engine bytes.
    pub(crate) fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            TableValueRef::Node(node) => {
                buf.push(tag::NODE);
                node.encode(buf);
            }
            TableValueRef::Skey(skey) => {
                buf.push(tag::SKEY);
                buf.extend_from_slice(&skey.into_bytes());
            }
            TableValueRef::Unit => buf.push(tag::UNIT),
        }
    }

    /// Materializes an owned [`TableValue`], copying a packed blob (as a plain read
    /// always did) but moving the other payloads out.
    pub(crate) fn into_owned(self) -> TableValue {
        match self {
            TableValueRef::Node(node) => TableValue::Node(node.into_owned()),
            TableValueRef::Skey(skey) => TableValue::Skey(skey),
            TableValueRef::Unit => TableValue::Unit,
        }
    }
}

impl<'a> NodeRef<'a> {
    /// Decodes a node from `r`'s buffer, borrowing a packed blob rather than
    /// copying it. Uses the shared `node::tag` discriminant constants, so this and
    /// the owned node model stay in lock-step on the on-disk format.
    fn decode(r: &mut Reader<'a>) -> SdbResult<NodeRef<'a>> {
        match r.u8()? {
            node_tag::OBJECT => Ok(NodeRef::Object),
            node_tag::PACKED => {
                let root = NodeKind::from_tag(r.u8()?)?;
                let blob = r.bytes()?;

                Ok(NodeRef::Packed {
                    root,
                    blob,
                })
            }
            node_tag::LIST => {
                let count = r.u32()? as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(Skey::from_bytes(r.array()?));
                }

                Ok(NodeRef::List(Cow::Owned(items)))
            }
            node_tag::LEAF => Ok(NodeRef::Leaf(Cow::Owned(Scalar::decode(r)?))),
            other => Err(SdbError::Corrupt(format!("unknown node tag {other}"))),
        }
    }

    /// Serializes a node using the shared `node::tag` discriminant constants; a
    /// packed blob is written straight from its slice (one copy, into `buf`).
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            NodeRef::Object => buf.push(node_tag::OBJECT),
            NodeRef::List(items) => {
                buf.push(node_tag::LIST);
                codec::put_u32(buf, items.len() as u32);

                for key in items.iter() {
                    buf.extend_from_slice(&key.into_bytes());
                }
            }
            NodeRef::Leaf(scalar) => {
                buf.push(node_tag::LEAF);
                scalar.encode(buf);
            }
            NodeRef::Packed {
                root,
                blob,
            } => {
                buf.push(node_tag::PACKED);
                buf.push(root.as_tag());
                codec::put_bytes(buf, blob);
            }
        }
    }

    /// Materializes an owned [`Node`], copying only a packed blob.
    fn into_owned(self) -> Node {
        match self {
            NodeRef::Object => Node::Object,
            NodeRef::List(items) => Node::List(items.into_owned()),
            NodeRef::Leaf(scalar) => Node::Leaf(scalar.into_owned()),
            NodeRef::Packed {
                root,
                blob,
            } => Node::Packed {
                root,
                blob: blob.to_vec(),
            },
        }
    }
}

impl RedbValue for TableValue {
    type AsBytes<'a> = Vec<u8>;
    type SelfType<'a> = TableValueRef<'a>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> TableValueRef<'a>
    where
        Self: 'a, {
        TableValueRef::decode(data).expect("corrupted table value")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Vec<u8>
    where
        Self: 'b, {
        let mut buf = Vec::new();
        value.encode(&mut buf);

        buf
    }

    fn type_name() -> TypeName {
        TypeName::new("stratodb::TableValue")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_unknown_tags() {
        // Unknown leading (value) tag.
        assert!(matches!(TableValueRef::decode(&[0xFF]), Err(SdbError::Corrupt(_))));

        // Valid value tag, but an unknown inner node tag.
        assert!(matches!(
            TableValueRef::decode(&[tag::NODE, 0xFF]),
            Err(SdbError::Corrupt(_))
        ));
    }

    #[test]
    fn every_variant_roundtrips_through_borrowed_bytes() {
        let values = [
            TableValue::Node(Node::Object),
            TableValue::Node(Node::List(vec![Skey::ROOT])),
            TableValue::Node(Node::Leaf(Scalar::I32(1))),
            TableValue::Node(Node::Packed {
                root: NodeKind::Object,
                blob: vec![1, 2, 3],
            }),
            TableValue::Skey(Skey::ROOT),
            TableValue::Unit,
        ];

        for value in values {
            let mut buf = Vec::new();
            value.as_ref().encode(&mut buf);

            // `TableValue` has no `PartialEq`, so compare via a re-encode.
            let mut round = Vec::new();
            TableValueRef::decode(&buf)
                .unwrap()
                .into_owned()
                .as_ref()
                .encode(&mut round);

            assert_eq!(buf, round);
        }
    }
}
