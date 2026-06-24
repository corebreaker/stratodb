//! Tree operations over the shredded node model.
//!
//! Reads are generic over any readable engine table (so they work inside both
//! read and write transactions); mutations require a writable table.
//!
//! Two engine entries back every node: `Data(skey) -> Node` (the node itself,
//! keyed by its stable primary key) and `Path(path) -> skey` (the path index).
//! Node identity lives in the primary key, so moving a list element only rewrites
//! the path index for the moved subtree.

use crate::{
    engine::{TableKey, TableValue},
    error::{SdbError, SdbResult},
    node::{Node, NodeKind},
    path::{SPath, Segment},
    data::Scalar,
    Skey,
};

use redb::{ReadableTable, Table};
use std::collections::BTreeMap;

/// A writable engine table over StratoDB keys and values.
type DataTable<'txn> = Table<'txn, TableKey, TableValue>;

// --------------------------------------------------------------------------
// Reads (generic over readable tables)
// --------------------------------------------------------------------------

fn read_node<T: ReadableTable<TableKey, TableValue>>(t: &T, skey: Skey) -> SdbResult<Option<Node>> {
    match t.get(&TableKey::Data(skey))? {
        Some(guard) => match guard.value() {
            TableValue::Node(node) => Ok(Some(node)),
            _ => Err(SdbError::Corrupt("expected a node at a data key".into())),
        },
        None => Ok(None),
    }
}

fn resolve<T: ReadableTable<TableKey, TableValue>>(t: &T, path: &SPath) -> SdbResult<Option<Skey>> {
    match t.get(&TableKey::Path(path.clone()))? {
        Some(guard) => match guard.value() {
            TableValue::Skey(skey) => Ok(Some(skey)),
            _ => Err(SdbError::Corrupt("expected a primary key at a path key".into())),
        },
        None => Ok(None),
    }
}

/// Reads the scalar stored at `path`, if it is a leaf.
pub(crate) fn get_scalar<T: ReadableTable<TableKey, TableValue>>(t: &T, path: &SPath) -> SdbResult<Option<Scalar>> {
    let Some(skey) = resolve(t, path)? else {
        return Ok(None);
    };

    match read_node(t, skey)? {
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
    let Some(skey) = resolve(t, path)? else {
        return Ok(None);
    };

    Ok(read_node(t, skey)?.map(|node| node.kind()))
}

// --------------------------------------------------------------------------
// Low-level writes
// --------------------------------------------------------------------------

fn write_node(t: &mut DataTable<'_>, skey: Skey, node: &Node) -> SdbResult<()> {
    t.insert(&TableKey::Data(skey), &TableValue::Node(node.clone()))?;
    Ok(())
}

fn write_link(t: &mut DataTable<'_>, path: &SPath, skey: Skey) -> SdbResult<()> {
    t.insert(&TableKey::Path(path.clone()), &TableValue::Skey(skey))?;
    Ok(())
}

fn delete_node(t: &mut DataTable<'_>, skey: Skey) -> SdbResult<()> {
    t.remove(&TableKey::Data(skey))?;
    Ok(())
}

fn delete_link(t: &mut DataTable<'_>, path: &SPath) -> SdbResult<()> {
    t.remove(&TableKey::Path(path.clone()))?;
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
        cascade_delete(t, old, path)?;
    }

    let child_is_index = matches!(last, Segment::Index(_));
    let parent_skey = ensure_container(t, &parent_path, child_is_index)?;

    let leaf = Skey::generate();
    write_node(t, leaf, &Node::Leaf(scalar))?;
    write_link(t, path, leaf)?;
    attach_child(t, parent_skey, &parent_path, last, leaf)?;
    Ok(())
}

fn put_root_scalar(t: &mut DataTable<'_>, scalar: Scalar) -> SdbResult<()> {
    let root = SPath::root();
    if let Some(old) = resolve(&*t, &root)? {
        cascade_delete(t, old, &root)?;
    }

    let leaf = Skey::generate();
    write_node(t, leaf, &Node::Leaf(scalar))?;
    write_link(t, &root, leaf)?;
    Ok(())
}

/// Removes the subtree at `path`, returning whether anything was removed.
pub(crate) fn remove_path(t: &mut DataTable<'_>, path: &SPath) -> SdbResult<bool> {
    let Some(skey) = resolve(&*t, path)? else {
        return Ok(false);
    };

    cascade_delete(t, skey, path)?;

    if let Some((parent_path, last)) = path.split_last()
        && let Some(parent_skey) = resolve(&*t, &parent_path)?
    {
        detach_child(t, parent_skey, &parent_path, last)?;
    }
    Ok(true)
}

/// Ensures a container node exists at `path` (creating object/list ancestors as
/// needed) and returns its primary key. `child_is_index` selects the kind to
/// create when `path` itself must be created.
fn ensure_container(t: &mut DataTable<'_>, path: &SPath, child_is_index: bool) -> SdbResult<Skey> {
    if path.is_root() {
        return ensure_root(t, child_is_index);
    }

    if let Some(skey) = resolve(&*t, path)? {
        let node = read_node(&*t, skey)?.ok_or_else(|| {
            const MSG: &str = "path link points to a missing node";

            SdbError::Corrupt(MSG.into())
        })?;

        return verify_container(node.kind(), path, child_is_index).map(|()| skey);
    }

    let (parent_path, last) = path.split_last().expect("a non-root path has a parent");
    let parent_skey = ensure_container(t, &parent_path, matches!(last, Segment::Index(_)))?;

    let skey = Skey::generate();
    let node = empty_container(child_is_index);
    write_node(t, skey, &node)?;
    write_link(t, path, skey)?;
    attach_child(t, parent_skey, &parent_path, last, skey)?;
    Ok(skey)
}

fn ensure_root(t: &mut DataTable<'_>, child_is_index: bool) -> SdbResult<Skey> {
    let root = SPath::root();
    if let Some(skey) = resolve(&*t, &root)? {
        let node = read_node(&*t, skey)?.ok_or_else(|| {
            const MSG: &str = "root link points to a missing node";

            SdbError::Corrupt(MSG.into())
        })?;

        return verify_container(node.kind(), &root, child_is_index).map(|()| skey);
    }

    let skey = Skey::generate();
    write_node(t, skey, &empty_container(child_is_index))?;
    write_link(t, &root, skey)?;
    Ok(skey)
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
    parent_skey: Skey,
    parent_path: &SPath,
    last: &Segment,
    child: Skey,
) -> SdbResult<()> {
    let mut node = read_node(&*t, parent_skey)?.ok_or_else(|| {
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
        (Node::Object(_), Segment::Index(_)) => {
            return Err(unexpected(parent_path, "list", "object"));
        }
        (Node::List(_), Segment::Name(_)) => {
            return Err(unexpected(parent_path, "object", "list"));
        }
        (Node::Leaf(_), _) => {
            return Err(unexpected(parent_path, "container", "leaf"));
        }
    }
    write_node(t, parent_skey, &node)?;
    Ok(())
}

/// Unlinks the final segment from `parent`, updating the parent node. Removing a
/// list element shifts following elements down and rewrites their path index.
fn detach_child(t: &mut DataTable<'_>, parent_skey: Skey, parent_path: &SPath, last: &Segment) -> SdbResult<()> {
    let mut node = read_node(&*t, parent_skey)?.ok_or_else(|| {
        const MSG: &str = "missing parent node while detaching";

        SdbError::Corrupt(MSG.into())
    })?;

    let mut shifted: Option<(usize, Vec<Skey>)> = None;
    match (&mut node, last) {
        (Node::Object(map), Segment::Name(name)) => {
            map.remove(name);
        }
        (Node::List(items), Segment::Index(index)) => {
            let index = *index as usize;
            if index < items.len() {
                items.remove(index);
            }

            let from = index.min(items.len());
            shifted = Some((from, items[from..].to_vec()));
        }
        (Node::Object(_), Segment::Index(_)) => return Err(unexpected(parent_path, "list", "object")),
        (Node::List(_), Segment::Name(_)) => return Err(unexpected(parent_path, "object", "list")),
        (Node::Leaf(_), _) => return Err(unexpected(parent_path, "container", "leaf")),
    }
    write_node(t, parent_skey, &node)?;

    if let Some((from, tail)) = shifted {
        for (offset, child) in tail.into_iter().enumerate() {
            let new_index = (from + offset) as u64;
            let old_path = parent_path.child_index(new_index + 1);
            let new_path = parent_path.child_index(new_index);

            reindex_subtree(t, child, &old_path, &new_path)?;
        }
    }
    Ok(())
}

/// Moves the path index of the subtree rooted at `root` from `old_root` to
/// `new_root`. Primary keys are stable, so only the path index changes.
fn reindex_subtree(t: &mut DataTable<'_>, root: Skey, old_root: &SPath, new_root: &SPath) -> SdbResult<()> {
    let mut stack = vec![(root, old_root.clone(), new_root.clone())];
    while let Some((skey, old_path, new_path)) = stack.pop() {
        delete_link(t, &old_path)?;
        write_link(t, &new_path, skey)?;
        if let Some(node) = read_node(&*t, skey)? {
            match node {
                Node::Object(map) => {
                    for (name, child) in map {
                        stack.push((child, old_path.child_name(name.clone()), new_path.child_name(name)));
                    }
                }
                Node::List(items) => {
                    for (i, child) in items.into_iter().enumerate() {
                        let i = i as u64;

                        stack.push((child, old_path.child_index(i), new_path.child_index(i)));
                    }
                }
                Node::Leaf(_) => {}
            }
        }
    }
    Ok(())
}

/// Deletes the subtree rooted at `skey` (its node entry and path index, plus
/// those of all descendants).
fn cascade_delete(t: &mut DataTable<'_>, skey: Skey, path: &SPath) -> SdbResult<()> {
    let mut stack = vec![(skey, path.clone())];
    while let Some((skey, path)) = stack.pop() {
        if let Some(node) = read_node(&*t, skey)? {
            match node {
                Node::Object(map) => {
                    for (name, child) in map {
                        stack.push((child, path.child_name(name)));
                    }
                }
                Node::List(items) => {
                    for (i, child) in items.into_iter().enumerate() {
                        stack.push((child, path.child_index(i as u64)));
                    }
                }
                Node::Leaf(_) => {}
            }
        }

        delete_node(t, skey)?;
        delete_link(t, &path)?;
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
