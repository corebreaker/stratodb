//! The node-storage backend abstraction.
//!
//! Tree operations ([`crate::tree`]) are written against these traits rather than
//! a concrete engine table, so the very same logic drives two stores:
//!
//! - the **engine table** (redb), holding the live tree — containers, child links and packed entities;
//! - an **in-memory mini node-table** ([`MemNodes`]), holding one packed entity's subtree while it is being built
//!   ([`store`](crate::txn::WriteTxn::store)), read back ([`load`](crate::txn::ReadTxn::load)) or edited in place.
//!
//! A packed entity is serialized as exactly such a mini node-table (see
//! [`MemNodes::to_blob`] / [`MemNodes::from_blob`]), so packing and unpacking is
//! just (de)serializing a `MemNodes` and the shredded tree logic is reused
//! verbatim on both sides.

use super::{TableKey, TableValue};
use crate::{
    codec::{self, Reader},
    error::SdbResult,
    node::Node,
    node::NodeKind,
    Skey,
};

use redb::{ReadableTable, Table};
use std::collections::BTreeMap;

/// An iterator over engine entries from a lower bound, in ascending key order.
pub(crate) type NodeIter<'a> = Box<dyn Iterator<Item = SdbResult<(TableKey, TableValue)>> + 'a>;

/// Read access to a node store: point lookup plus an ordered forward scan.
pub(crate) trait ReadNodes {
    fn fetch(&self, key: &TableKey) -> SdbResult<Option<TableValue>>;

    /// Entries at or after `lower`, in ascending key order. Callers stop as soon as
    /// a key falls outside the range they care about (e.g. a child block).
    fn scan_from(&self, lower: &TableKey) -> SdbResult<NodeIter<'_>>;
}

/// Read/write access to a node store.
pub(crate) trait WriteNodes: ReadNodes {
    fn put(&mut self, key: TableKey, value: TableValue) -> SdbResult<()>;

    fn delete(&mut self, key: &TableKey) -> SdbResult<()>;
}

// -- redb engine tables --------------------------------------------------------

impl<T: ReadableTable<TableKey, TableValue>> ReadNodes for T {
    fn fetch(&self, key: &TableKey) -> SdbResult<Option<TableValue>> {
        Ok(self.get(key)?.map(|guard| guard.value()))
    }

    fn scan_from(&self, lower: &TableKey) -> SdbResult<NodeIter<'_>> {
        let range = self.range(lower.clone()..)?;

        Ok(Box::new(range.map(|item| {
            let (key, value) = item?;

            Ok((key.value(), value.value()))
        })))
    }
}

impl WriteNodes for Table<'_, TableKey, TableValue> {
    fn put(&mut self, key: TableKey, value: TableValue) -> SdbResult<()> {
        self.insert(&key, &value)?;
        Ok(())
    }

    fn delete(&mut self, key: &TableKey) -> SdbResult<()> {
        self.remove(key)?;
        Ok(())
    }
}

// -- in-memory mini node-table -------------------------------------------------

/// An in-memory node store ordered exactly like the engine (by the
/// order-preserving [`TableKey`] encoding), so the tree logic — including the
/// child-block range scans — behaves identically over it.
#[derive(Clone, Debug, Default)]
pub(crate) struct MemNodes {
    entries: BTreeMap<TableKey, TableValue>,
}

impl MemNodes {
    /// An empty store (its root node is created by the first write).
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The kind of the subtree root (node [`Skey::ROOT`] within this store), used
    /// to tag the packed node so its kind is known without decoding the blob.
    pub(crate) fn root_kind(&self) -> SdbResult<NodeKind> {
        match self.entries.get(&TableKey::Data(Skey::ROOT)) {
            Some(TableValue::Node(node)) => Ok(node.kind()),
            // A never-written entity (e.g. an empty struct) packs as an empty object.
            _ => Ok(NodeKind::Object),
        }
    }

    /// Serializes the store into a packed-entity blob: a length-prefixed list of
    /// `(key, value)` byte pairs, in key order.
    pub(crate) fn to_blob(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        codec::put_u32(&mut buf, self.entries.len() as u32);

        for (key, value) in &self.entries {
            codec::put_bytes(&mut buf, &key.encode());
            codec::put_bytes(&mut buf, &value.encode());
        }

        buf
    }

    /// Rebuilds a store from a blob written by [`to_blob`](Self::to_blob).
    pub(crate) fn from_blob(blob: &[u8]) -> SdbResult<MemNodes> {
        let mut r = Reader::new(blob);
        let count = r.u32()? as usize;

        let mut entries = BTreeMap::new();
        for _ in 0..count {
            let key = TableKey::decode(r.bytes()?)?;
            let value = TableValue::decode(r.bytes()?)?;
            entries.insert(key, value);
        }

        Ok(MemNodes {
            entries,
        })
    }

    /// Packs this store into a [`Node::Packed`] carrying its root kind and blob.
    pub(crate) fn into_packed(self) -> SdbResult<Node> {
        Ok(Node::Packed {
            root: self.root_kind()?,
            blob: self.to_blob(),
        })
    }

    /// Consumes the store, yielding its `(key, value)` entries in key order. Used
    /// to spill a packed entity back into the live table (unpacking).
    pub(crate) fn into_entries(self) -> impl Iterator<Item = (TableKey, TableValue)> {
        self.entries.into_iter()
    }
}

impl ReadNodes for MemNodes {
    fn fetch(&self, key: &TableKey) -> SdbResult<Option<TableValue>> {
        Ok(self.entries.get(key).cloned())
    }

    fn scan_from(&self, lower: &TableKey) -> SdbResult<NodeIter<'_>> {
        Ok(Box::new(
            self.entries
                .range(lower.clone()..)
                .map(|(key, value)| Ok((key.clone(), value.clone()))),
        ))
    }
}

impl WriteNodes for MemNodes {
    fn put(&mut self, key: TableKey, value: TableValue) -> SdbResult<()> {
        self.entries.insert(key, value);
        Ok(())
    }

    fn delete(&mut self, key: &TableKey) -> SdbResult<()> {
        self.entries.remove(key);
        Ok(())
    }
}
