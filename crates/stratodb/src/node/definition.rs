use super::NodeKind;
use crate::{
    codec::{self, Reader},
    error::{SdbError, SdbResult},
    data::Scalar,
    Skey,
};

use std::collections::BTreeMap;

mod tag {
    pub(super) const OBJECT: u8 = 0;
    pub(super) const LIST: u8 = 1;
    pub(super) const LEAF: u8 = 2;
}

/// A stored node: either a container (object/list) of child keys, or a leaf.
#[derive(Clone, Debug)]
pub(crate) enum Node {
    /// An object: an ordered map from field name to child key.
    Object(BTreeMap<String, Skey>),
    /// A list: a zero-based sequence of child keys.
    List(Vec<Skey>),
    /// A leaf: a single scalar value.
    Leaf(Scalar),
}

impl Node {
    pub(crate) fn kind(&self) -> NodeKind {
        match self {
            Node::Object(_) => NodeKind::Object,
            Node::List(_) => NodeKind::List,
            Node::Leaf(_) => NodeKind::Leaf,
        }
    }

    pub(crate) fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Node::Object(map) => {
                buf.push(tag::OBJECT);
                codec::put_u32(buf, map.len() as u32);

                for (name, key) in map {
                    codec::put_bytes(buf, name.as_bytes());
                    buf.extend_from_slice(&key.to_bytes());
                }
            }
            Node::List(items) => {
                buf.push(tag::LIST);
                codec::put_u32(buf, items.len() as u32);

                for key in items {
                    buf.extend_from_slice(&key.to_bytes());
                }
            }
            Node::Leaf(scalar) => {
                buf.push(tag::LEAF);
                scalar.encode(buf);
            }
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> SdbResult<Node> {
        match r.u8()? {
            tag::OBJECT => {
                let count = r.u32()? as usize;
                let mut map = BTreeMap::new();
                for _ in 0..count {
                    let name_bytes = r.bytes()?;
                    let name = std::str::from_utf8(name_bytes)
                        .map_err(|_| SdbError::Corrupt("invalid utf-8 in object field".into()))?
                        .to_string();
                    let key = Skey::from_bytes(r.array()?);
                    map.insert(name, key);
                }

                Ok(Node::Object(map))
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
