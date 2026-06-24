//! The composite key type stored in a StratoDB data table.
//!
//! [`TableKey`]'s leading discriminant partitions the key space into contiguous
//! `Data` and `Index` ranges, so a single engine table can hold both nodes and
//! index entries. The encoding is order-preserving, so key comparison reduces to
//! a bytewise comparison of the encoded bytes.

use crate::{
    codec::{self, Reader},
    error::{SdbError, SdbResult},
    key::{IndexId, Skey},
};

use redb::{Key as RedbKey, TypeName, Value as RedbValue};
use std::cmp::Ordering;

mod tag {
    pub(super) const DATA: u8 = 0;
    pub(super) const INDEX: u8 = 2;
}

/// The key of an entry in a StratoDB data table.
#[derive(Clone, Debug)]
pub(crate) enum TableKey {
    /// A node, addressed by its primary key.
    Data(Skey),
    /// An index entry. The exact column layout is finalized by the index
    /// milestone; `entity` is `None` for unique indexes (the entity is stored in
    /// the value instead).
    Index {
        id:     IndexId,
        cols:   Vec<u8>,
        entity: Option<Skey>,
    },
}

impl TableKey {
    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            TableKey::Data(skey) => {
                buf.push(tag::DATA);
                buf.extend_from_slice(&skey.to_bytes());
            }
            TableKey::Index {
                id,
                cols,
                entity,
            } => {
                buf.push(tag::INDEX);
                codec::put_u32(&mut buf, id.0);
                codec::put_bytes(&mut buf, cols);
                match entity {
                    Some(skey) => {
                        buf.push(1);
                        buf.extend_from_slice(&skey.to_bytes());
                    }
                    None => buf.push(0),
                }
            }
        }
        buf
    }

    fn decode(data: &[u8]) -> SdbResult<TableKey> {
        let mut r = Reader::new(data);
        let key = match r.u8()? {
            tag::DATA => TableKey::Data(Skey::from_bytes(r.array()?)),
            tag::INDEX => {
                let id = IndexId(r.u32()?);
                let cols = r.bytes()?.to_vec();
                let entity = match r.u8()? {
                    0 => None,
                    _ => Some(Skey::from_bytes(r.array()?)),
                };

                TableKey::Index {
                    id,
                    cols,
                    entity,
                }
            }
            other => return Err(SdbError::Corrupt(format!("unknown table key tag {other}"))),
        };

        Ok(key)
    }
}

impl RedbValue for TableKey {
    type AsBytes<'a> = Vec<u8>;
    type SelfType<'a> = TableKey;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> TableKey
    where
        Self: 'a, {
        TableKey::decode(data).expect("corrupted table key")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Vec<u8>
    where
        Self: 'b, {
        value.encode()
    }

    fn type_name() -> TypeName {
        TypeName::new("stratodb::TableKey")
    }
}

impl RedbKey for TableKey {
    fn compare(data1: &[u8], data2: &[u8]) -> Ordering {
        // The encoding is order-preserving by construction, so a bytewise
        // comparison yields the intended total order.
        data1.cmp(data2)
    }
}
