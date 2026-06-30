//! The composite key type stored in a StratoDB data table.
//!
//! [`TableKey`]'s leading discriminant partitions the key space into contiguous
//! `Data` and index ranges, so a single engine table holds both nodes and index
//! entries. The encoding is order-preserving, so key comparison reduces to a
//! bytewise comparison of the encoded bytes.
//!
//! Index keys are laid out so a **prefix scan** works: `tag · id · cols`, where
//! `cols` is the raw, self-delimiting order-preserving encoding of the indexed
//! columns (see [`crate::index::ordered`]) with no length prefix (a length
//! prefix would sort by length before content and defeat prefix queries). A
//! non-unique index appends the 16-byte entity key as a tie-breaker (the entity
//! lives in the key); a unique index stores the entity in the value instead, so
//! its key ends after `cols`. The two cases use distinct tags so the decoder
//! knows whether a trailing entity key is present.

use crate::{
    codec,
    error::{SdbError, SdbResult},
    index::IndexId,
    key::Skey,
};

use redb::{Key as RedbKey, TypeName, Value as RedbValue};
use std::cmp::Ordering;

mod tag {
    pub(super) const DATA: u8 = 0;
    /// Object child link: `parent · name`.
    pub(super) const CHILD: u8 = 1;
    /// Non-unique index entry: `id · cols · entity`.
    pub(super) const INDEX_DUP: u8 = 2;
    /// Unique index entry: `id · cols` (entity is in the value).
    pub(super) const INDEX_UNIQUE: u8 = 3;
}

/// The key of an entry in a StratoDB data table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TableKey {
    /// A node, addressed by its primary key.
    Data(Skey),
    /// An object's child link: the child key under `parent` for field `name`.
    /// Encoded as `parent(16) · name`, so every child of one parent forms one
    /// contiguous, name-sorted key block — a child is a point lookup, and a
    /// parent's whole child set is a single forward range scan. The child key is
    /// held in the value ([`TableValue::Skey`](crate::engine::TableValue)).
    Child { parent: Skey, name: String },
    /// An index entry. `cols` is the order-preserving encoding of the indexed
    /// columns; `entity` is `Some` for non-unique indexes (the entity lives in
    /// the key) and `None` for unique ones (the entity is stored in the value).
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
                buf.extend_from_slice(&skey.into_bytes());
            }
            TableKey::Child {
                parent,
                name,
            } => {
                buf.push(tag::CHILD);
                buf.extend_from_slice(&parent.into_bytes());
                buf.extend_from_slice(name.as_bytes());
            }
            TableKey::Index {
                id,
                cols,
                entity: Some(entity),
            } => {
                buf.push(tag::INDEX_DUP);
                codec::put_u32(&mut buf, id.0);
                buf.extend_from_slice(cols);
                buf.extend_from_slice(&entity.into_bytes());
            }
            TableKey::Index {
                id,
                cols,
                entity: None,
            } => {
                buf.push(tag::INDEX_UNIQUE);
                codec::put_u32(&mut buf, id.0);
                buf.extend_from_slice(cols);
            }
        }
        buf
    }

    fn decode(data: &[u8]) -> SdbResult<TableKey> {
        let (&tag, rest) = data
            .split_first()
            .ok_or_else(|| SdbError::Corrupt("empty table key".into()))?;

        match tag {
            tag::DATA => {
                let bytes = rest
                    .try_into()
                    .map_err(|_| SdbError::Corrupt("malformed data key".into()))?;

                Ok(TableKey::Data(Skey::from_bytes(bytes)))
            }
            tag::CHILD => {
                // parent(16) · name(var)
                if rest.len() < 16 {
                    return Err(SdbError::Corrupt("child key missing parent".into()));
                }

                let (parent, name) = rest.split_at(16);
                let parent = Skey::from_bytes(parent.try_into().expect("16 bytes"));
                let name = std::str::from_utf8(name)
                    .map_err(|_| SdbError::Corrupt("invalid utf-8 in child link name".into()))?
                    .to_string();

                Ok(TableKey::Child {
                    parent,
                    name,
                })
            }
            tag::INDEX_DUP => {
                // id(4) · cols(var) · entity(16)
                let (id, body) = split_id(rest)?;
                if body.len() < 16 {
                    return Err(SdbError::Corrupt("non-unique index key missing entity".into()));
                }

                let (cols, entity) = body.split_at(body.len() - 16);
                let entity = entity
                    .try_into()
                    .map_err(|_| SdbError::Corrupt("non-unique index entity: expected 16 bytes".into()))?;

                let entity = Skey::from_bytes(entity);

                Ok(TableKey::Index {
                    id,
                    cols: cols.to_vec(),
                    entity: Some(entity),
                })
            }
            tag::INDEX_UNIQUE => {
                // id(4) · cols(var)
                let (id, cols) = split_id(rest)?;

                Ok(TableKey::Index {
                    id,
                    cols: cols.to_vec(),
                    entity: None,
                })
            }
            other => Err(SdbError::Corrupt(format!("unknown table key tag {other}"))),
        }
    }
}

/// Splits a leading big-endian `IndexId` off an index key body.
fn split_id(body: &[u8]) -> SdbResult<(IndexId, &[u8])> {
    if body.len() < 4 {
        return Err(SdbError::Corrupt("index key missing id".into()));
    }

    let (id, rest) = body.split_at(4);
    let id = IndexId(u32::from_be_bytes(id.try_into().expect("4 bytes")));

    Ok((id, rest))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(key: TableKey) {
        let bytes = key.encode();
        assert_eq!(TableKey::decode(&bytes).expect("decode"), key);
    }

    fn skey(n: u128) -> Skey {
        Skey::from_bytes(*uuid::Uuid::from_u128(n).as_bytes())
    }

    #[test]
    fn roundtrips_every_variant() {
        roundtrip(TableKey::Data(skey(7)));
        roundtrip(TableKey::Child {
            parent: skey(7),
            name:   String::from("field"),
        });
        roundtrip(TableKey::Child {
            parent: skey(7),
            name:   String::new(),
        });
        roundtrip(TableKey::Index {
            id:     IndexId(3),
            cols:   vec![1, 2, 0, 3],
            entity: Some(skey(42)),
        });
        roundtrip(TableKey::Index {
            id:     IndexId(3),
            cols:   vec![],
            entity: Some(skey(42)),
        });
        roundtrip(TableKey::Index {
            id:     IndexId(9),
            cols:   vec![5, 6, 7],
            entity: None,
        });
    }

    /// Bytewise comparison must reproduce the intended order: data keys before
    /// index keys, then by id, then by column bytes, then by entity.
    #[test]
    fn encoding_is_order_preserving() {
        let ordered = [
            TableKey::Data(skey(0)),
            TableKey::Data(skey(u128::MAX)),
            // Child entries form a contiguous block between data and index keys,
            // grouped by parent and then sorted by name.
            TableKey::Child {
                parent: skey(0),
                name:   String::new(),
            },
            TableKey::Child {
                parent: skey(0),
                name:   String::from("age"),
            },
            TableKey::Child {
                parent: skey(0),
                name:   String::from("name"),
            },
            TableKey::Child {
                parent: skey(1),
                name:   String::from("age"),
            },
            TableKey::Index {
                id:     IndexId(0),
                cols:   vec![1],
                entity: Some(skey(0)),
            },
            TableKey::Index {
                id:     IndexId(0),
                cols:   vec![1],
                entity: Some(skey(1)),
            },
            TableKey::Index {
                id:     IndexId(0),
                cols:   vec![2],
                entity: Some(skey(0)),
            },
            TableKey::Index {
                id:     IndexId(1),
                cols:   vec![0],
                entity: Some(skey(0)),
            },
        ];

        for pair in ordered.windows(2) {
            assert!(
                pair[0].encode() < pair[1].encode(),
                "{:?} should encode below {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    /// A shorter column run sorts before a longer one sharing its prefix — the
    /// property a prefix scan relies on (no length prefix to defeat it).
    #[test]
    fn column_prefixes_sort_before_extensions() {
        let short = TableKey::Index {
            id:     IndexId(0),
            cols:   vec![1, 2],
            entity: Some(skey(0)),
        };
        let long = TableKey::Index {
            id:     IndexId(0),
            cols:   vec![1, 2, 0],
            entity: Some(skey(0)),
        };

        assert!(short.encode() < long.encode());
    }
}
