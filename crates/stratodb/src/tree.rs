//! Tree operations over the shredded node model.
//!
//! The single source of truth is `Data(key) -> Node`: an object node maps field
//! names to child keys, a list node holds an ordered vector of child keys. A path
//! is resolved by **walking** from the fixed root key ([`Skey::ROOT`]), following
//! those links — there is no separate path index, so a structural list edit only
//! rewrites the affected list node (keys are stable; positions are implicit in
//! the vector's order).
//!
//! Reads are generic over any readable engine table (so they work inside both
//! read and write transactions); mutations require a writable table.

use crate::{
    data::Scalar,
    engine::{TableKey, TableValue},
    error::{SdbError, SdbResult},
    node::{Node, NodeKind},
    path::{SPath, Segment},
    Skey,
};

use redb::{ReadableTable, Table};
use std::collections::BTreeMap;

/// A writable engine table over StratoDB keys and values.
type DataTable<'txn> = Table<'txn, TableKey, TableValue>;

// --------------------------------------------------------------------------
// Reads (generic over readable tables)
// --------------------------------------------------------------------------

pub(crate) fn read_node<T: ReadableTable<TableKey, TableValue>>(t: &T, key: Skey) -> SdbResult<Option<Node>> {
    match t.get(&TableKey::Data(key))? {
        Some(guard) => match guard.value() {
            TableValue::Node(node) => Ok(Some(node)),
            _ => Err(SdbError::Corrupt("expected a node at a data key".into())),
        },
        None => Ok(None),
    }
}

/// Resolves `path` to a primary key by walking from the root, or `None` if the
/// table is empty or any segment along the way is absent.
pub(crate) fn resolve<T: ReadableTable<TableKey, TableValue>>(t: &T, path: &SPath) -> SdbResult<Option<Skey>> {
    if read_node(t, Skey::ROOT)?.is_none() {
        return Ok(None);
    }

    let mut key = Skey::ROOT;
    for seg in path.segments() {
        let Some(child) = child_key(t, key, seg)? else {
            return Ok(None);
        };

        key = child;
    }

    Ok(Some(key))
}

/// Reads the child key under `parent` for `seg`, if `parent` is a container that
/// holds it.
pub(crate) fn child_key<T: ReadableTable<TableKey, TableValue>>(
    t: &T,
    parent: Skey,
    seg: &Segment,
) -> SdbResult<Option<Skey>> {
    let Some(node) = read_node(t, parent)? else {
        return Ok(None);
    };

    let child = match (node, seg) {
        (Node::Object(map), Segment::Name(name)) => map.get(name).copied(),
        (Node::List(items), Segment::Index(index)) => items.get(*index as usize).copied(),
        _ => None,
    };

    Ok(child)
}

/// Reads the scalar stored at `path`, if it is a leaf.
pub(crate) fn get_scalar<T: ReadableTable<TableKey, TableValue>>(t: &T, path: &SPath) -> SdbResult<Option<Scalar>> {
    let Some(key) = resolve(t, path)? else {
        return Ok(None);
    };

    match read_node(t, key)? {
        Some(Node::Leaf(scalar)) => Ok(Some(scalar)),
        Some(other) => Err(SdbError::UnexpectedNode {
            path:     path.clone(),
            expected: "leaf",
            found:    other.kind().as_str(),
        }),
        None => Err(SdbError::Corrupt("path resolves to a missing node".into())),
    }
}

/// Reports the kind of node stored at `path`, if any.
pub(crate) fn kind<T: ReadableTable<TableKey, TableValue>>(t: &T, path: &SPath) -> SdbResult<Option<NodeKind>> {
    let Some(key) = resolve(t, path)? else {
        return Ok(None);
    };

    Ok(read_node(t, key)?.map(|node| node.kind()))
}

/// Reads the scalar held by the leaf node `key`.
pub(crate) fn scalar_at<T: ReadableTable<TableKey, TableValue>>(t: &T, key: Skey) -> SdbResult<Scalar> {
    match read_node(t, key)? {
        Some(Node::Leaf(scalar)) => Ok(scalar),
        Some(other) => Err(SdbError::Corrupt(format!(
            "node {key} is a {}, expected a leaf",
            other.kind().as_str()
        ))),
        None => Err(SdbError::Corrupt(format!("no node for key {key}"))),
    }
}

/// Reports the kind of the node `key`, if it exists.
pub(crate) fn kind_of<T: ReadableTable<TableKey, TableValue>>(t: &T, key: Skey) -> SdbResult<Option<NodeKind>> {
    Ok(read_node(t, key)?.map(|node| node.kind()))
}

/// Returns the length of the list node `key`.
pub(crate) fn list_len<T: ReadableTable<TableKey, TableValue>>(t: &T, key: Skey) -> SdbResult<usize> {
    match read_node(t, key)? {
        Some(Node::List(items)) => Ok(items.len()),
        Some(other) => Err(SdbError::Corrupt(format!(
            "node {key} is a {}, expected a list",
            other.kind().as_str()
        ))),
        None => Err(SdbError::Corrupt(format!("no node for key {key}"))),
    }
}

/// Returns the field names of the object node `key`, in sorted (`BTreeMap`) order.
pub(crate) fn object_keys<T: ReadableTable<TableKey, TableValue>>(t: &T, key: Skey) -> SdbResult<Vec<String>> {
    match read_node(t, key)? {
        Some(Node::Object(map)) => Ok(map.into_keys().collect()),
        Some(other) => Err(SdbError::Corrupt(format!(
            "node {key} is a {}, expected an object",
            other.kind().as_str()
        ))),
        None => Err(SdbError::Corrupt(format!("no node for key {key}"))),
    }
}

// --------------------------------------------------------------------------
// Low-level writes
// --------------------------------------------------------------------------

fn write_node(t: &mut DataTable<'_>, key: Skey, node: &Node) -> SdbResult<()> {
    t.insert(&TableKey::Data(key), &TableValue::Node(node.clone()))?;
    Ok(())
}

fn delete_node(t: &mut DataTable<'_>, key: Skey) -> SdbResult<()> {
    t.remove(&TableKey::Data(key))?;
    Ok(())
}

// --------------------------------------------------------------------------
// High-level operations
// --------------------------------------------------------------------------

/// Stores `scalar` at `path`, replacing any existing subtree there and creating
/// container ancestors as needed.
pub(crate) fn put_scalar(t: &mut DataTable<'_>, path: &SPath, scalar: Scalar) -> SdbResult<()> {
    let Some((parent_path, last)) = path.split_last() else {
        return put_root_scalar(t, scalar);
    };

    // Replace semantics: drop the old subtree at this path first.
    if let Some(old) = resolve(&*t, path)? {
        cascade_delete(t, old)?;
    }

    let child_is_index = matches!(last, Segment::Index(_));
    let parent_key = ensure_container(t, &parent_path, child_is_index)?;

    let leaf = Skey::generate();
    write_node(t, leaf, &Node::Leaf(scalar))?;
    attach_child(t, parent_key, &parent_path, last, leaf)?;
    Ok(())
}

fn put_root_scalar(t: &mut DataTable<'_>, scalar: Scalar) -> SdbResult<()> {
    if read_node(&*t, Skey::ROOT)?.is_some() {
        cascade_delete(t, Skey::ROOT)?;
    }

    write_node(t, Skey::ROOT, &Node::Leaf(scalar))
}

/// Removes the subtree at `path`, returning whether anything was removed.
pub(crate) fn remove_path(t: &mut DataTable<'_>, path: &SPath) -> SdbResult<bool> {
    let Some(key) = resolve(&*t, path)? else {
        return Ok(false);
    };

    cascade_delete(t, key)?;

    if let Some((parent_path, last)) = path.split_last()
        && let Some(parent_key) = resolve(&*t, &parent_path)?
    {
        detach_child(t, parent_key, &parent_path, last)?;
    }

    Ok(true)
}

/// Ensures a container node exists at `path` (creating object/list ancestors as
/// needed) and returns its primary key. `child_is_index` selects the kind to
/// create when `path` itself must be created.
pub(crate) fn ensure_container(t: &mut DataTable<'_>, path: &SPath, child_is_index: bool) -> SdbResult<Skey> {
    if path.is_root() {
        return ensure_root(t, child_is_index);
    }

    if let Some(key) = resolve(&*t, path)? {
        let node = read_node(&*t, key)?.ok_or_else(|| {
            const MSG: &str = "resolved path points to a missing node";

            SdbError::Corrupt(MSG.into())
        })?;

        return verify_container(node.kind(), path, child_is_index).map(|()| key);
    }

    let (parent_path, last) = path.split_last().ok_or_else(|| {
        let msg = format!("a non-root path has a parent: {path}");

        SdbError::InvalidPath(msg)
    })?;

    let parent_key = ensure_container(t, &parent_path, matches!(last, Segment::Index(_)))?;

    let key = Skey::generate();
    write_node(t, key, &empty_container(child_is_index))?;
    attach_child(t, parent_key, &parent_path, last, key)?;
    Ok(key)
}

fn ensure_root(t: &mut DataTable<'_>, child_is_index: bool) -> SdbResult<Skey> {
    if let Some(node) = read_node(&*t, Skey::ROOT)? {
        return verify_container(node.kind(), &SPath::root(), child_is_index).map(|()| Skey::ROOT);
    }

    write_node(t, Skey::ROOT, &empty_container(child_is_index))?;
    Ok(Skey::ROOT)
}

fn empty_container(is_list: bool) -> Node {
    if is_list {
        Node::List(Vec::new())
    } else {
        Node::Object(BTreeMap::new())
    }
}

fn verify_container(kind: NodeKind, path: &SPath, child_is_index: bool) -> SdbResult<()> {
    let expected = if child_is_index {
        NodeKind::List
    } else {
        NodeKind::Object
    };

    if kind == expected {
        Ok(())
    } else {
        Err(SdbError::UnexpectedNode {
            path:     path.clone(),
            expected: expected.as_str(),
            found:    kind.as_str(),
        })
    }
}

/// Links `child` under `parent` at the final segment, updating the parent node.
fn attach_child(
    t: &mut DataTable<'_>,
    parent_key: Skey,
    parent_path: &SPath,
    last: &Segment,
    child: Skey,
) -> SdbResult<()> {
    let mut node = read_node(&*t, parent_key)?.ok_or_else(|| {
        const MSG: &str = "missing parent node while attaching";

        SdbError::Corrupt(MSG.into())
    })?;

    match (&mut node, last) {
        (Node::Object(map), Segment::Name(name)) => {
            map.insert(name.clone(), child);
        }
        (Node::List(items), Segment::Index(index)) => {
            let index = *index;
            let len = items.len() as u64;

            if index < len {
                items[index as usize] = child;
            } else if index == len {
                items.push(child);
            } else {
                return Err(SdbError::IndexOutOfRange {
                    path: parent_path.child_index(index),
                    index,
                    len,
                });
            }
        }
        (Node::Object(_), Segment::Index(_)) => return Err(unexpected(parent_path, "list", "object")),
        (Node::List(_), Segment::Name(_)) => return Err(unexpected(parent_path, "object", "list")),
        (Node::Leaf(_), _) => return Err(unexpected(parent_path, "container", "leaf")),
    }

    write_node(t, parent_key, &node)?;
    Ok(())
}

/// Unlinks the final segment from `parent`, updating the parent node. Removing a
/// list element shifts the following elements left in the vector — and because
/// paths are walked (not indexed), nothing else needs rewriting.
fn detach_child(t: &mut DataTable<'_>, parent_key: Skey, parent_path: &SPath, last: &Segment) -> SdbResult<()> {
    let mut node = read_node(&*t, parent_key)?.ok_or_else(|| {
        const MSG: &str = "missing parent node while detaching";

        SdbError::Corrupt(MSG.into())
    })?;

    match (&mut node, last) {
        (Node::Object(map), Segment::Name(name)) => {
            map.remove(name);
        }
        (Node::List(items), Segment::Index(index)) => {
            let index = *index as usize;
            if index < items.len() {
                items.remove(index);
            }
        }
        (Node::Object(_), Segment::Index(_)) => return Err(unexpected(parent_path, "list", "object")),
        (Node::List(_), Segment::Name(_)) => return Err(unexpected(parent_path, "object", "list")),
        (Node::Leaf(_), _) => return Err(unexpected(parent_path, "container", "leaf")),
    }

    write_node(t, parent_key, &node)?;
    Ok(())
}

/// Reorders a list element from `from` to `to` within the list node `list_key`.
/// Only the list node's vector changes; the moved subtree keeps its key.
pub(crate) fn list_move(t: &mut DataTable<'_>, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
    let mut node = read_node(&*t, list_key)?.ok_or_else(|| {
        const MSG: &str = "missing list node while moving an element";

        SdbError::Corrupt(MSG.into())
    })?;

    match &mut node {
        Node::List(items) => {
            if from >= items.len() {
                return Err(SdbError::IndexOutOfRange {
                    path:  SPath::root(),
                    index: from as u64,
                    len:   items.len() as u64,
                });
            }

            let moved = items.remove(from);
            let to = to.min(items.len());
            items.insert(to, moved);
        }
        other => {
            return Err(SdbError::Corrupt(format!(
                "node {list_key} is a {}, expected a list",
                other.kind().as_str()
            )));
        }
    }

    write_node(t, list_key, &node)?;
    Ok(())
}

/// Deletes the subtree rooted at `key` (its node entry and all descendants').
fn cascade_delete(t: &mut DataTable<'_>, key: Skey) -> SdbResult<()> {
    let mut stack = vec![key];
    while let Some(key) = stack.pop() {
        if let Some(node) = read_node(&*t, key)? {
            match node {
                Node::Object(map) => stack.extend(map.into_values()),
                Node::List(items) => stack.extend(items),
                Node::Leaf(_) => {}
            }
        }

        delete_node(t, key)?;
    }

    Ok(())
}

fn unexpected(path: &SPath, expected: &'static str, found: &'static str) -> SdbError {
    SdbError::UnexpectedNode {
        path: path.clone(),
        expected,
        found,
    }
}
