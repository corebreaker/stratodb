//! The [`JsonExporter`] / [`YamlExporter`] traits and their implementations.
//!
//! Both a [`ReadTxn`] (rendering a stored subtree) and a [`Value`] (rendering an
//! in-memory subtree) export through the same path-addressed contract, so the
//! two writers in `json`/`yaml` serve both.

use super::{json::to_json, yaml::to_yaml};
use crate::{
    data::Scalar,
    error::{SdbError, SdbResult},
    path::{IntoPath, SPath},
    txn::ReadTxn,
    Value,
};

/// Renders a document to JSON.
///
/// Implemented by [`ReadTxn`] (rendering the table's subtree at `path`) and by
/// [`Value`] (rendering the in-memory subtree at `path`). The root path renders
/// the whole document.
pub trait JsonExporter {
    /// Exports the subtree at `path` as a JSON document. `indent` selects the
    /// layout: `None` yields compact JSON (no whitespace), `Some(n)` pretty-prints
    /// it with `n` spaces of indentation per nesting level.
    ///
    /// Object fields come out in sorted order. Scalars without a native JSON form
    /// take a textual one: dates and times as ISO 8601 / RFC 3339, a UUID
    /// hyphenated, raw bytes as Base64, a duration as its number of seconds, the
    /// non-finite floats (`NaN`, `±∞`) as `null`. A `path` that resolves to no
    /// node errors with [`PathNotFound`](crate::SdbError::PathNotFound).
    fn export_to_json(&self, path: impl IntoPath, indent: Option<usize>) -> SdbResult<String>;
}

/// Renders a document to YAML, in block style.
///
/// Implemented by [`ReadTxn`] and [`Value`], like [`JsonExporter`].
pub trait YamlExporter {
    /// Exports the subtree at `path` as a YAML document (block style). Object
    /// fields come out in sorted order and every string is double-quoted; scalar
    /// rendering and the missing-path behaviour match
    /// [`JsonExporter::export_to_json`].
    fn export_to_yaml(&self, path: impl IntoPath) -> SdbResult<String>;
}

impl JsonExporter for ReadTxn {
    /// The root of an empty table exports as `null`; otherwise see [`JsonExporter`].
    fn export_to_json(&self, path: impl IntoPath, indent: Option<usize>) -> SdbResult<String> {
        Ok(to_json(&txn_value(self, &path.into_path()?)?, indent))
    }
}

impl YamlExporter for ReadTxn {
    /// The root of an empty table exports as `null`; otherwise see [`YamlExporter`].
    fn export_to_yaml(&self, path: impl IntoPath) -> SdbResult<String> {
        Ok(to_yaml(&txn_value(self, &path.into_path()?)?))
    }
}

impl JsonExporter for Value {
    fn export_to_json(&self, path: impl IntoPath, indent: Option<usize>) -> SdbResult<String> {
        Ok(to_json(navigate(self, &path.into_path()?)?, indent))
    }
}

impl YamlExporter for Value {
    fn export_to_yaml(&self, path: impl IntoPath) -> SdbResult<String> {
        Ok(to_yaml(navigate(self, &path.into_path()?)?))
    }
}

/// The [`Value`] a read transaction renders for `base`: the resolved subtree, or
/// — for the root of an empty table — a `null` leaf. Any other absent path is
/// [`PathNotFound`](crate::SdbError::PathNotFound).
fn txn_value(txn: &ReadTxn, base: &SPath) -> SdbResult<Value> {
    match txn.read_value(base)? {
        Some(value) => Ok(value),
        None if base.is_root() => Ok(Value::Leaf(Scalar::Null)),
        None => Err(SdbError::PathNotFound(base.clone())),
    }
}

/// Resolves `base` within an in-memory [`Value`] to the addressed subtree, or
/// [`PathNotFound`](crate::SdbError::PathNotFound) if a segment leads nowhere. The
/// root path returns the whole value.
fn navigate<'v>(value: &'v Value, base: &SPath) -> SdbResult<&'v Value> {
    value.subtree(base).ok_or_else(|| SdbError::PathNotFound(base.clone()))
}
