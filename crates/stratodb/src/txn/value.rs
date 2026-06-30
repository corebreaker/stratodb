//! Loading and storing the dynamic [`Value`] tree on transactions.
//!
//! `load_value` walks a resolved subtree into an owned [`Value`] — a faithful,
//! lossless mirror of the stored scalars; `store_value` decomposes a [`Value`]
//! back into nodes, replacing whatever was at the path and maintaining indexes
//! exactly as a typed `store` would.

use super::{read::ReadTxn, write::WriteTxn, write::segment_path};
use crate::{
    access::{BoundCursor, WriteCursor, Writer},
    engine::{TableKey, TableValue},
    error::{SdbError, SdbResult},
    node::Node,
    path::{IntoPath, SPath, Segment},
    tree,
    Skey,
    Value,
};

use redb::ReadableTable;
use std::{cell::RefCell, collections::BTreeMap};

impl ReadTxn {
    /// Loads the subtree at `path` as a [`Value`], or `None` if nothing resolves
    /// there. The result is a faithful copy: each leaf keeps its exact stored
    /// [`Scalar`](crate::data::Scalar).
    pub fn load_value(&self, path: impl IntoPath) -> SdbResult<Option<Value>> {
        self.read_value(&path.into_path()?)
    }

    /// Walks the subtree at `base` into a [`Value`], or `None` if `base` resolves
    /// to no node. Shared by [`load_value`](Self::load_value) and the exporters.
    pub(crate) fn read_value(&self, base: &SPath) -> SdbResult<Option<Value>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        match tree::resolve(table, base)? {
            Some(key) => Ok(Some(value_at(table, key)?)),
            None => Ok(None),
        }
    }
}

impl WriteTxn {
    /// Stores `value` at `path`, replacing any existing subtree there and keeping
    /// indexes consistent. The dynamic counterpart of [`store`](Self::store): an
    /// object or list materializes its container (even when empty) and a leaf its
    /// scalar.
    pub fn store_value(&self, path: impl IntoPath, value: &Value) -> SdbResult<()> {
        let base = path.into_path()?;

        let Some((parent_path, last)) = base.split_last() else {
            // Storing at the table root: nothing to anchor to, so take the plain path.
            let cursor = WriteCursor::new(self);
            cursor.remove(&base)?;

            return write_value(&cursor, value, &base);
        };

        // One index-maintenance bracket and one table handle for the whole subtree,
        // exactly like the typed `store`: see [`WriteTxn::store`].
        let rel = segment_path(last);
        self.reindex_around(&base, |table| {
            tree::remove_path(table, &base)?;
            let anchor = tree::ensure_container(table, &parent_path, matches!(last, Segment::Index(_)))?;

            let cell = RefCell::new(table);
            write_value(&BoundCursor::new(&cell, anchor), value, &rel)
        })
    }
}

/// Reads the node at `key` and converts its subtree into a [`Value`]. A dangling
/// child key is corruption — every key held by a container must resolve to a node.
fn value_at<T: ReadableTable<TableKey, TableValue>>(table: &T, key: Skey) -> SdbResult<Value> {
    let value = match tree::read_node(table, key)? {
        Some(Node::Leaf(scalar)) => Value::Leaf(scalar),
        Some(Node::List(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for child in items {
                out.push(value_at(table, child)?);
            }

            Value::List(out)
        }
        Some(Node::Object) => {
            let mut out = BTreeMap::new();
            for (name, child) in tree::object_children(table, key)? {
                out.insert(name, value_at(table, child)?);
            }

            Value::Node(out)
        }
        None => {
            return Err(SdbError::Corrupt(
                "value: a child key resolves to a missing node".into(),
            ));
        }
    };

    Ok(value)
}

/// Writes `value` at `at`, recursing into containers. Assumes the location was
/// already cleared, so containers start empty and a leaf replaces cleanly.
fn write_value<W: Writer>(writer: &W, value: &Value, at: &SPath) -> SdbResult<()> {
    match value {
        Value::Leaf(scalar) => writer.put_scalar(at, scalar.clone()),
        Value::List(items) => {
            writer.ensure_container(at, true)?;
            for (index, item) in items.iter().enumerate() {
                write_value(writer, item, &at.child_index(index as u64))?;
            }

            Ok(())
        }
        Value::Node(entries) => {
            writer.ensure_container(at, false)?;
            for (name, item) in entries {
                write_value(writer, item, &at.child_name(name.as_str()))?;
            }

            Ok(())
        }
    }
}
