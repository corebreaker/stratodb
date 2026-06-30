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

// --------------------------------------------------------------------------
// Object child links (one engine entry per `(parent, name)`)
// --------------------------------------------------------------------------

/// The child key under object `parent` for field `name`, if present. A single
/// point lookup — no parent node is read or deserialized.
fn object_child<T: ReadableTable<TableKey, TableValue>>(t: &T, parent: Skey, name: &str) -> SdbResult<Option<Skey>> {
    let key = TableKey::Child {
        parent,
        name: name.to_string(),
    };

    match t.get(&key)? {
        Some(guard) => match guard.value() {
            TableValue::Skey(child) => Ok(Some(child)),
            _ => Err(SdbError::Corrupt("object child link is not a key".into())),
        },
        None => Ok(None),
    }
}

/// Every `(name, child key)` of object `parent`, in ascending name order — one
/// forward range scan over the parent's contiguous child-link block.
pub(crate) fn object_children<T: ReadableTable<TableKey, TableValue>>(
    t: &T,
    parent: Skey,
) -> SdbResult<Vec<(String, Skey)>> {
    let lower = TableKey::Child {
        parent,
        name: String::new(),
    };

    let mut out = Vec::new();
    for item in t.range(lower..)? {
        let (key, value) = item?;
        match key.value() {
            TableKey::Child {
                parent: p,
                name,
            } if p == parent => match value.value() {
                TableValue::Skey(child) => out.push((name, child)),
                _ => return Err(SdbError::Corrupt("object child link is not a key".into())),
            },
            _ => break,
        }
    }

    Ok(out)
}

/// Links `child` under object `parent` at `name`, replacing any existing link.
fn put_object_child(t: &mut DataTable<'_>, parent: Skey, name: &str, child: Skey) -> SdbResult<()> {
    let key = TableKey::Child {
        parent,
        name: name.to_string(),
    };

    t.insert(&key, &TableValue::Skey(child))?;
    Ok(())
}

/// Unlinks `name` from object `parent`, if present.
fn remove_object_child(t: &mut DataTable<'_>, parent: Skey, name: &str) -> SdbResult<()> {
    let key = TableKey::Child {
        parent,
        name: name.to_string(),
    };

    t.remove(&key)?;
    Ok(())
}

/// Removes every child link of object `parent` and returns the child keys it held
/// (so the caller can recurse into them). The parent node itself is untouched.
fn take_object_children(t: &mut DataTable<'_>, parent: Skey) -> SdbResult<Vec<Skey>> {
    // redb's range borrows the table, so collect the links before removing them.
    let links: Vec<(String, Skey)> = object_children(&*t, parent)?;

    let mut children = Vec::with_capacity(links.len());
    for (name, child) in links {
        remove_object_child(t, parent, &name)?;
        children.push(child);
    }

    Ok(children)
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
    match seg {
        // An object child is a direct point lookup; the parent node is never read.
        // A name on a non-object parent simply has no such child link, so this
        // returns `None` — the same answer the old whole-node match gave.
        Segment::Name(name) => object_child(t, parent, name),
        Segment::Index(index) => match read_node(t, parent)? {
            Some(Node::List(items)) => Ok(items.get(*index as usize).copied()),
            _ => Ok(None),
        },
    }
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
        Some(Node::Object) => Ok(object_children(t, key)?.into_iter().map(|(name, _)| name).collect()),
        Some(other) => Err(SdbError::Corrupt(format!(
            "node {key} is a {}, expected an object",
            other.kind().as_str()
        ))),
        None => Err(SdbError::Corrupt(format!("no node for key {key}"))),
    }
}

/// Returns the child keys of the container node `key`, in node order (object
/// fields sorted by name, list elements by position). A leaf or a missing node
/// has no children. Used to expand an index pattern's `*` over every child.
pub(crate) fn children<T: ReadableTable<TableKey, TableValue>>(t: &T, key: Skey) -> SdbResult<Vec<Skey>> {
    match read_node(t, key)? {
        Some(Node::Object) => Ok(object_children(t, key)?.into_iter().map(|(_, child)| child).collect()),
        Some(Node::List(items)) => Ok(items),
        Some(Node::Leaf(_)) | None => Ok(Vec::new()),
    }
}

/// Resolves `rel` starting from `base` rather than the root, returning the key it
/// lands on, or `None` if any segment along the way is absent. Index maintenance
/// uses this to read an entity's column values relative to the entity's own key.
pub(crate) fn resolve_from<T: ReadableTable<TableKey, TableValue>>(
    t: &T,
    base: Skey,
    rel: &SPath,
) -> SdbResult<Option<Skey>> {
    let mut key = base;
    for seg in rel.segments() {
        match child_key(t, key, seg)? {
            Some(child) => key = child,
            None => return Ok(None),
        }
    }

    Ok(Some(key))
}

/// Reads the scalar at leaf node `key`, or `None` if `key` is missing or is not a
/// leaf. Unlike [`scalar_at`], a container is not an error — an index column that
/// points at a non-leaf simply has no scalar (it indexes as `Null`).
pub(crate) fn leaf_scalar_opt<T: ReadableTable<TableKey, TableValue>>(t: &T, key: Skey) -> SdbResult<Option<Scalar>> {
    match read_node(t, key)? {
        Some(Node::Leaf(scalar)) => Ok(Some(scalar)),
        _ => Ok(None),
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

    let child_is_index = matches!(last, Segment::Index(_));
    let parent_key = ensure_container(t, &parent_path, child_is_index)?;

    // Replace semantics: drop the old subtree at this child, if any. Looking it up
    // through the now-resolved parent avoids re-walking the whole path from root.
    if let Some(old) = child_key(&*t, parent_key, last)? {
        cascade_delete(t, old)?;
    }

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

// --------------------------------------------------------------------------
// Root-anchored operations
//
// These mirror `put_scalar` / `ensure_container` / `remove_path` but walk from an
// arbitrary, already-existing `root` key instead of `Skey::ROOT`. A whole-entity
// `store` resolves the entity's parent once and then drives every field through
// these, so a field no longer re-walks the path from the table root on each write.
// --------------------------------------------------------------------------

/// Ensures a container exists at `rel` (relative to the existing node `root`),
/// creating intermediate containers as needed, and returns its key.
pub(crate) fn ensure_container_rel(
    t: &mut DataTable<'_>,
    root: Skey,
    rel: &SPath,
    child_is_index: bool,
) -> SdbResult<Skey> {
    let Some((parent_rel, last)) = rel.split_last() else {
        let node = read_node(&*t, root)?.ok_or_else(|| {
            const MSG: &str = "relative-store root points to a missing node";

            SdbError::Corrupt(MSG.into())
        })?;

        return verify_container(node.kind(), &SPath::root(), child_is_index).map(|()| root);
    };

    if let Some(key) = resolve_from(&*t, root, rel)? {
        let node = read_node(&*t, key)?.ok_or_else(|| {
            const MSG: &str = "resolved relative path points to a missing node";

            SdbError::Corrupt(MSG.into())
        })?;

        return verify_container(node.kind(), rel, child_is_index).map(|()| key);
    }

    let parent_key = ensure_container_rel(t, root, &parent_rel, matches!(last, Segment::Index(_)))?;

    let key = Skey::generate();
    write_node(t, key, &empty_container(child_is_index))?;
    attach_child(t, parent_key, &parent_rel, last, key)?;
    Ok(key)
}

/// Stores `scalar` at `rel` (relative to the existing node `root`), replacing any
/// existing subtree there and creating container ancestors as needed.
pub(crate) fn put_scalar_rel(t: &mut DataTable<'_>, root: Skey, rel: &SPath, scalar: Scalar) -> SdbResult<()> {
    let Some((parent_rel, last)) = rel.split_last() else {
        // Setting the anchored root itself to a leaf — drop whatever it held first.
        if read_node(&*t, root)?.is_some() {
            cascade_delete(t, root)?;
        }

        return write_node(t, root, &Node::Leaf(scalar));
    };

    let child_is_index = matches!(last, Segment::Index(_));
    let parent_key = ensure_container_rel(t, root, &parent_rel, child_is_index)?;

    if let Some(old) = child_key(&*t, parent_key, last)? {
        cascade_delete(t, old)?;
    }

    let leaf = Skey::generate();
    write_node(t, leaf, &Node::Leaf(scalar))?;
    attach_child(t, parent_key, &parent_rel, last, leaf)?;
    Ok(())
}

/// Removes the subtree at `rel` (relative to `root`), returning whether anything
/// was removed.
pub(crate) fn remove_rel(t: &mut DataTable<'_>, root: Skey, rel: &SPath) -> SdbResult<bool> {
    let Some(key) = resolve_from(&*t, root, rel)? else {
        return Ok(false);
    };

    cascade_delete(t, key)?;

    if let Some((parent_rel, last)) = rel.split_last()
        && let Some(parent_key) = resolve_from(&*t, root, &parent_rel)?
    {
        detach_child(t, parent_key, &parent_rel, last)?;
    }

    Ok(true)
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
    if is_list { Node::List(Vec::new()) } else { Node::Object }
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

/// Links `child` under `parent` at the final segment. An object child is written
/// as its own child-link entry (no parent rewrite); a list element updates the
/// parent list node's vector.
fn attach_child(
    t: &mut DataTable<'_>,
    parent_key: Skey,
    parent_path: &SPath,
    last: &Segment,
    child: Skey,
) -> SdbResult<()> {
    match last {
        // An object link is a single point write; the parent node is never read or
        // rewritten, so a wide object no longer costs O(siblings) per child.
        Segment::Name(name) => put_object_child(t, parent_key, name, child),
        Segment::Index(index) => {
            let mut node = read_node(&*t, parent_key)?.ok_or_else(|| {
                const MSG: &str = "missing parent node while attaching";

                SdbError::Corrupt(MSG.into())
            })?;

            match &mut node {
                Node::List(items) => {
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
                Node::Object => return Err(unexpected(parent_path, "list", "object")),
                Node::Leaf(_) => return Err(unexpected(parent_path, "container", "leaf")),
            }

            write_node(t, parent_key, &node)
        }
    }
}

/// Unlinks the final segment from `parent`. An object child drops its child-link
/// entry; a list element shifts the following elements left in the vector — and
/// because paths are walked (not indexed), nothing else needs rewriting.
fn detach_child(t: &mut DataTable<'_>, parent_key: Skey, parent_path: &SPath, last: &Segment) -> SdbResult<()> {
    match last {
        Segment::Name(name) => remove_object_child(t, parent_key, name),
        Segment::Index(index) => {
            let mut node = read_node(&*t, parent_key)?.ok_or_else(|| {
                const MSG: &str = "missing parent node while detaching";

                SdbError::Corrupt(MSG.into())
            })?;

            match &mut node {
                Node::List(items) => {
                    let index = *index as usize;
                    if index < items.len() {
                        items.remove(index);
                    }
                }
                Node::Object => return Err(unexpected(parent_path, "list", "object")),
                Node::Leaf(_) => return Err(unexpected(parent_path, "container", "leaf")),
            }

            write_node(t, parent_key, &node)
        }
    }
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

/// Swaps the elements at `i` and `j` within the list node `list_key`. Only the
/// list node's vector changes; the swapped subtrees keep their keys.
pub(crate) fn list_swap(t: &mut DataTable<'_>, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
    let mut node = read_node(&*t, list_key)?.ok_or_else(|| {
        const MSG: &str = "missing list node while swapping elements";

        SdbError::Corrupt(MSG.into())
    })?;

    match &mut node {
        Node::List(items) => {
            let len = items.len();
            if i >= len || j >= len {
                return Err(SdbError::IndexOutOfRange {
                    path:  SPath::root(),
                    index: i.max(j) as u64,
                    len:   len as u64,
                });
            }

            items.swap(i, j);
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

/// Removes every child of the container node `key` (cascading), leaving an empty
/// container of the same kind. Errors if `key` is a leaf.
pub(crate) fn clear_children(t: &mut DataTable<'_>, key: Skey) -> SdbResult<()> {
    let node = read_node(&*t, key)?.ok_or_else(|| {
        const MSG: &str = "missing node while clearing children";

        SdbError::Corrupt(MSG.into())
    })?;

    match node {
        Node::Object => {
            for child in take_object_children(t, key)? {
                cascade_delete(t, child)?;
            }

            // The object marker stays in place; only its child links are gone.
        }
        Node::List(items) => {
            for child in items {
                cascade_delete(t, child)?;
            }

            write_node(t, key, &Node::List(Vec::new()))?;
        }
        Node::Leaf(_) => {
            return Err(SdbError::Corrupt(format!("node {key} is a leaf, expected a container")));
        }
    }

    Ok(())
}

/// Deletes the subtree rooted at `key` (its node entry and all descendants').
fn cascade_delete(t: &mut DataTable<'_>, key: Skey) -> SdbResult<()> {
    let mut stack = vec![key];
    while let Some(key) = stack.pop() {
        if let Some(node) = read_node(&*t, key)? {
            match node {
                // Detaches and collects the object's child links as we descend, so
                // no orphan link entry survives the deletion.
                Node::Object => stack.extend(take_object_children(t, key)?),
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
