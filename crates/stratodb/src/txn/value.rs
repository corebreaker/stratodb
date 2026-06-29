//! Loading and storing the dynamic [`Value`] tree on transactions.
//!
//! `load_value` walks a resolved subtree into an owned [`Value`] — a faithful,
//! lossless mirror of the stored scalars; `store_value` decomposes a [`Value`]
//! back into nodes, replacing whatever was at the path and maintaining indexes
//! exactly as a typed `store` would.

use super::{read::ReadTxn, write::WriteTxn};
use crate::{
    access::{WriteCursor, Writer},
    engine::{TableKey, TableValue},
    error::{SdbError, SdbResult},
    node::Node,
    path::{IntoPath, SPath},
    tree,
    Skey,
    Value,
};

use redb::ReadableTable;
use std::collections::BTreeMap;

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

        match tree::resolve(&table, base)? {
            Some(key) => match tree::read_node(&table, key)? {
                Some(node) => Ok(Some(node_to_value(&table, node)?)),
                None => Err(SdbError::Corrupt(
                    "value: resolved path points to a missing node".into(),
                )),
            },
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
        let cursor = WriteCursor::new(self);
        cursor.remove(&base)?;

        write_value(&cursor, value, &base)
    }
}

/// Converts an already-read node and its descendants into a [`Value`].
fn node_to_value<T: ReadableTable<TableKey, TableValue>>(table: &T, node: Node) -> SdbResult<Value> {
    let value = match node {
        Node::Leaf(scalar) => Value::Leaf(scalar),
        Node::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for key in items {
                out.push(child_value(table, key)?);
            }

            Value::List(out)
        }
        Node::Object(map) => {
            let mut out = BTreeMap::new();
            for (name, key) in map {
                out.insert(name, child_value(table, key)?);
            }

            Value::Node(out)
        }
    };

    Ok(value)
}

/// Reads the node at `key` and converts its subtree. A dangling child key is
/// corruption — every key held by a container must resolve to a node.
fn child_value<T: ReadableTable<TableKey, TableValue>>(table: &T, key: Skey) -> SdbResult<Value> {
    match tree::read_node(table, key)? {
        Some(node) => node_to_value(table, node),
        None => Err(SdbError::Corrupt(
            "value: a child key resolves to a missing node".into(),
        )),
    }
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
