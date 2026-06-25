//! The composite value type stored in a StratoDB data table.
//!
//! A [`TableValue`] is the payload an engine entry maps to: a node (for `Data`
//! keys), a referenced primary key (the entity, for unique index entries), or
//! nothing (for non-unique index entries, whose entity lives in the key).

use crate::{
    codec::Reader,
    error::{SdbError, SdbResult},
    key::Skey,
    node::Node,
};

use redb::{TypeName, Value as RedbValue};

mod tag {
    pub(super) const NODE: u8 = 0;
    pub(super) const SKEY: u8 = 1;
    pub(super) const UNIT: u8 = 2;
}

/// The value of an entry in a StratoDB data table.
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
    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            TableValue::Node(node) => {
                buf.push(tag::NODE);
                node.encode(&mut buf);
            }
            TableValue::Skey(skey) => {
                buf.push(tag::SKEY);
                buf.extend_from_slice(&skey.to_bytes());
            }
            TableValue::Unit => buf.push(tag::UNIT),
        }

        buf
    }

    fn decode(data: &[u8]) -> SdbResult<TableValue> {
        let mut r = Reader::new(data);
        let value = match r.u8()? {
            tag::NODE => TableValue::Node(Node::decode(&mut r)?),
            tag::SKEY => TableValue::Skey(Skey::from_bytes(r.array()?)),
            tag::UNIT => TableValue::Unit,
            other => return Err(SdbError::Corrupt(format!("unknown table value tag {other}"))),
        };

        Ok(value)
    }
}

impl RedbValue for TableValue {
    type AsBytes<'a> = Vec<u8>;
    type SelfType<'a> = TableValue;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> TableValue
    where
        Self: 'a, {
        TableValue::decode(data).expect("corrupted table value")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Vec<u8>
    where
        Self: 'b, {
        value.encode()
    }

    fn type_name() -> TypeName {
        TypeName::new("stratodb::TableValue")
    }
}
