//! Tree operations over the shredded node model.
//!
//! The single source of truth is `Data(key) -> Node`: an object node's
//! `(name -> child key)` links are separate `Child` entries, a list node holds an
//! ordered vector of child keys, a leaf holds a scalar, and a packed entity holds
//! a whole subtree in one value. A path is resolved by **walking** from a root key
//! ([`Skey::ROOT`] for the table), following those links.
//!
//! Every operation is generic over a [`ReadNodes`] / [`WriteNodes`] backend, so
//! the same logic drives both the live engine table and the in-memory mini
//! node-table that backs a packed entity (see [`crate::engine::backend`]). A
//! packed entity is **terminal** to these functions: they never descend into its
//! blob (its children are addressed inside it); descent is the cursor's job, which
//! decodes the blob into a `MemNodes` and runs these same functions over it.

use crate::{
    data::Scalar,
    engine::{MemNodes, ReadNodes, TableKey, TableValue, WriteNodes},
    error::{SdbError, SdbResult},
    node::{Node, NodeKind},
    path::{SPath, Segment},
    Skey,
};

// --------------------------------------------------------------------------
// Reads
// --------------------------------------------------------------------------

pub(crate) fn read_node<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<Option<Node>> {
    match b.fetch(&TableKey::Data(key))? {
        Some(TableValue::Node(node)) => Ok(Some(node)),
        Some(_) => Err(SdbError::Corrupt("expected a node at a data key".into())),
        None => Ok(None),
    }
}

// --------------------------------------------------------------------------
// Object child links (one engine entry per `(parent, name)`)
// --------------------------------------------------------------------------

/// The child key under object `parent` for field `name`, if present. A single
/// point lookup — no parent node is read or deserialized.
fn object_child<B: ReadNodes>(b: &B, parent: Skey, name: &str) -> SdbResult<Option<Skey>> {
    let key = TableKey::Child {
        parent,
        name: name.to_string(),
    };

    match b.fetch(&key)? {
        Some(TableValue::Skey(child)) => Ok(Some(child)),
        Some(_) => Err(SdbError::Corrupt("object child link is not a key".into())),
        None => Ok(None),
    }
}

/// Every `(name, child key)` of object `parent`, in ascending name order — one
/// forward range scan over the parent's contiguous child-link block.
pub(crate) fn object_children<B: ReadNodes>(b: &B, parent: Skey) -> SdbResult<Vec<(String, Skey)>> {
    let lower = TableKey::Child {
        parent,
        name: String::new(),
    };

    let mut out = Vec::new();
    for item in b.scan_from(&lower)? {
        let (key, value) = item?;
        match key {
            TableKey::Child {
                parent: p,
                name,
            } if p == parent => match value {
                TableValue::Skey(child) => out.push((name, child)),
                _ => return Err(SdbError::Corrupt("object child link is not a key".into())),
            },
            _ => break,
        }
    }

    Ok(out)
}

/// Links `child` under object `parent` at `name`, replacing any existing link.
fn put_object_child<B: WriteNodes>(b: &mut B, parent: Skey, name: &str, child: Skey) -> SdbResult<()> {
    let key = TableKey::Child {
        parent,
        name: name.to_string(),
    };

    b.put(key, TableValue::Skey(child))
}

/// Unlinks `name` from object `parent`, if present.
fn remove_object_child<B: WriteNodes>(b: &mut B, parent: Skey, name: &str) -> SdbResult<()> {
    let key = TableKey::Child {
        parent,
        name: name.to_string(),
    };

    b.delete(&key)
}

/// Removes every child link of object `parent` and returns the child keys it held
/// (so the caller can recurse into them). The parent node itself is untouched.
fn take_object_children<B: WriteNodes>(b: &mut B, parent: Skey) -> SdbResult<Vec<Skey>> {
    // The scan borrows the backend, so collect the links before removing them.
    let links: Vec<(String, Skey)> = object_children(&*b, parent)?;

    let mut children = Vec::with_capacity(links.len());
    for (name, child) in links {
        remove_object_child(b, parent, &name)?;
        children.push(child);
    }

    Ok(children)
}

/// Resolves `path` to a primary key by walking from the root, or `None` if the
/// store is empty or any segment along the way is absent. Stops at a packed
/// entity: a path that lands on one resolves to it, but a path that continues
/// *into* one returns `None` here (the cursor descends via the blob).
pub(crate) fn resolve<B: ReadNodes>(b: &B, path: &SPath) -> SdbResult<Option<Skey>> {
    resolve_from_checked(b, Skey::ROOT, path, true)
}

/// Where a path lands in the live store.
pub(crate) enum Located {
    /// Nothing resolves at the path.
    Missing,
    /// A plain main-table node (container or standalone leaf), addressed by key.
    Plain(Skey),
    /// A location at or inside a packed entity: the entity's key plus the path
    /// *within its blob* (`rel` empty when the path lands on the entity itself).
    Packed { entity: Skey, rel: SPath },
}

/// Walks `path` from the table root, reporting whether it lands on a plain node or
/// at/inside a packed entity (so the caller can decode that entity's blob and
/// continue there). A packed entity is detected before descending through it.
pub(crate) fn locate<B: ReadNodes>(b: &B, path: &SPath) -> SdbResult<Located> {
    if read_node(b, Skey::ROOT)?.is_none() {
        return Ok(Located::Missing);
    }

    let segs = path.segments();
    let mut key = Skey::ROOT;
    let mut i = 0;
    loop {
        if matches!(read_node(b, key)?, Some(Node::Packed { .. })) {
            return Ok(Located::Packed {
                entity: key,
                rel:    SPath::from_segments(&segs[i..]),
            });
        }

        if i == segs.len() {
            return Ok(Located::Plain(key));
        }

        match child_key(b, key, &segs[i])? {
            Some(child) => {
                key = child;
                i += 1;
            }
            None => return Ok(Located::Missing),
        }
    }
}

/// Reads the child key under `parent` for `seg`, if `parent` holds it. A packed
/// entity is terminal here (its children live in its blob), so this returns
/// `None` for any segment under one.
pub(crate) fn child_key<B: ReadNodes>(b: &B, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
    match seg {
        // An object child is a direct point lookup; the parent node is never read.
        // A name on a non-object parent simply has no such child link.
        Segment::Name(name) => object_child(b, parent, name),
        Segment::Index(index) => match read_node(b, parent)? {
            Some(Node::List(items)) => Ok(items.get(*index as usize).copied()),
            _ => Ok(None),
        },
    }
}

/// Reads the scalar stored at `path`, if it is a leaf, descending transparently
/// into a packed entity's blob.
pub(crate) fn get_scalar<B: ReadNodes>(b: &B, path: &SPath) -> SdbResult<Option<Scalar>> {
    match locate(b, path)? {
        Located::Missing => Ok(None),
        Located::Plain(key) => leaf_at(b, key, path),
        Located::Packed {
            entity,
            rel,
        } => {
            let mem = decode_packed(b, entity)?;
            match resolve_from(&mem, Skey::ROOT, &rel)? {
                Some(key) => leaf_at(&mem, key, path),
                None => Ok(None),
            }
        }
    }
}

/// Reads the leaf scalar at `key`, erroring if the node there is not a leaf.
fn leaf_at<B: ReadNodes>(b: &B, key: Skey, path: &SPath) -> SdbResult<Option<Scalar>> {
    match read_node(b, key)? {
        Some(Node::Leaf(scalar)) => Ok(Some(scalar)),
        Some(other) => Err(SdbError::UnexpectedNode {
            path:     path.clone(),
            expected: "leaf",
            found:    other.kind().as_str(),
        }),
        None => Err(SdbError::Corrupt("path resolves to a missing node".into())),
    }
}

/// Reports the kind of node stored at `path`, if any, descending transparently
/// into a packed entity's blob.
pub(crate) fn kind<B: ReadNodes>(b: &B, path: &SPath) -> SdbResult<Option<NodeKind>> {
    match locate(b, path)? {
        Located::Missing => Ok(None),
        Located::Plain(key) => Ok(read_node(b, key)?.map(|node| node.kind())),
        Located::Packed {
            entity,
            rel,
        } => {
            let mem = decode_packed(b, entity)?;
            match resolve_from(&mem, Skey::ROOT, &rel)? {
                Some(key) => Ok(read_node(&mem, key)?.map(|node| node.kind())),
                None => Ok(None),
            }
        }
    }
}

/// Reads the scalar held by the leaf node `key`.
pub(crate) fn scalar_at<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<Scalar> {
    match read_node(b, key)? {
        Some(Node::Leaf(scalar)) => Ok(scalar),
        Some(other) => Err(SdbError::Corrupt(format!(
            "node {key} is a {}, expected a leaf",
            other.kind().as_str()
        ))),
        None => Err(SdbError::Corrupt(format!("no node for key {key}"))),
    }
}

/// Reports the kind of the node `key`, if it exists.
pub(crate) fn kind_of<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<Option<NodeKind>> {
    Ok(read_node(b, key)?.map(|node| node.kind()))
}

/// Returns the length of the list node `key`.
pub(crate) fn list_len<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<usize> {
    match read_node(b, key)? {
        Some(Node::List(items)) => Ok(items.len()),
        Some(other) => Err(SdbError::Corrupt(format!(
            "node {key} is a {}, expected a list",
            other.kind().as_str()
        ))),
        None => Err(SdbError::Corrupt(format!("no node for key {key}"))),
    }
}

/// Returns the field names of the object node `key`, in sorted (`BTreeMap`) order.
pub(crate) fn object_keys<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<Vec<String>> {
    match read_node(b, key)? {
        Some(Node::Object) => Ok(object_children(b, key)?.into_iter().map(|(name, _)| name).collect()),
        Some(other) => Err(SdbError::Corrupt(format!(
            "node {key} is a {}, expected an object",
            other.kind().as_str()
        ))),
        None => Err(SdbError::Corrupt(format!("no node for key {key}"))),
    }
}

/// Returns the child keys of the container node `key`, in node order (object
/// fields sorted by name, list elements by position). A leaf, a packed entity or
/// a missing node has no children here. Used to expand an index pattern's `*`.
pub(crate) fn children<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<Vec<Skey>> {
    match read_node(b, key)? {
        Some(Node::Object) => Ok(object_children(b, key)?.into_iter().map(|(_, child)| child).collect()),
        Some(Node::List(items)) => Ok(items),
        Some(Node::Leaf(_))
        | Some(Node::Packed {
            ..
        })
        | None => Ok(Vec::new()),
    }
}

/// Resolves `rel` starting from `base` rather than the root, returning the key it
/// lands on, or `None` if any segment along the way is absent. Index maintenance
/// uses this to read an entity's column values relative to the entity's own key.
pub(crate) fn resolve_from<B: ReadNodes>(b: &B, base: Skey, rel: &SPath) -> SdbResult<Option<Skey>> {
    resolve_from_checked(b, base, rel, false)
}

/// The shared walk for [`resolve`] and [`resolve_from`]. When `require_root`, an
/// empty store (no node at `base`) resolves to `None`.
fn resolve_from_checked<B: ReadNodes>(b: &B, base: Skey, rel: &SPath, require_root: bool) -> SdbResult<Option<Skey>> {
    if require_root && read_node(b, base)?.is_none() {
        return Ok(None);
    }

    let mut key = base;
    for seg in rel.segments() {
        let Some(child) = child_key(b, key, seg)? else {
            return Ok(None);
        };

        key = child;
    }

    Ok(Some(key))
}

/// Reads the scalar at leaf node `key`, or `None` if `key` is missing or is not a
/// leaf. Unlike [`scalar_at`], a container is not an error — an index column that
/// points at a non-leaf simply has no scalar (it indexes as `Null`).
pub(crate) fn leaf_scalar_opt<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<Option<Scalar>> {
    match read_node(b, key)? {
        Some(Node::Leaf(scalar)) => Ok(Some(scalar)),
        _ => Ok(None),
    }
}

// --------------------------------------------------------------------------
// Low-level writes
// --------------------------------------------------------------------------

fn write_node<B: WriteNodes>(b: &mut B, key: Skey, node: Node) -> SdbResult<()> {
    b.put(TableKey::Data(key), TableValue::Node(node))
}

fn delete_node<B: WriteNodes>(b: &mut B, key: Skey) -> SdbResult<()> {
    b.delete(&TableKey::Data(key))
}

// --------------------------------------------------------------------------
// High-level operations (anchored at `root`; `Skey::ROOT` for the whole table)
// --------------------------------------------------------------------------

/// Stores `scalar` at `path`, replacing any existing subtree there and creating
/// container ancestors as needed.
pub(crate) fn put_scalar<B: WriteNodes>(b: &mut B, path: &SPath, scalar: Scalar) -> SdbResult<()> {
    put_scalar_rel(b, Skey::ROOT, path, scalar)
}

/// Stores `scalar` at `rel` relative to the existing node `root`.
pub(crate) fn put_scalar_rel<B: WriteNodes>(b: &mut B, root: Skey, rel: &SPath, scalar: Scalar) -> SdbResult<()> {
    let Some((parent_rel, last)) = rel.split_last() else {
        return put_node_scalar(b, root, scalar);
    };

    let child_is_index = matches!(last, Segment::Index(_));
    let parent_key = ensure_container_rel(b, root, &parent_rel, child_is_index)?;

    // Replace semantics: drop the old subtree at this child, if any. Looking it up
    // through the now-resolved parent avoids re-walking the whole path from root.
    if let Some(old) = child_key(&*b, parent_key, last)? {
        cascade_delete(b, old)?;
    }

    let leaf = Skey::generate();
    write_node(b, leaf, Node::Leaf(scalar))?;
    attach_child(b, parent_key, &parent_rel, last, leaf)
}

/// Sets node `key` itself to a leaf scalar, dropping whatever subtree it held.
fn put_node_scalar<B: WriteNodes>(b: &mut B, key: Skey, scalar: Scalar) -> SdbResult<()> {
    if read_node(&*b, key)?.is_some() {
        cascade_delete(b, key)?;
    }

    write_node(b, key, Node::Leaf(scalar))
}

/// Removes the subtree at `path`, returning whether anything was removed.
pub(crate) fn remove_path<B: WriteNodes>(b: &mut B, path: &SPath) -> SdbResult<bool> {
    remove_rel(b, Skey::ROOT, path)
}

/// Removes the subtree at `rel` relative to `root`, returning whether anything was
/// removed.
pub(crate) fn remove_rel<B: WriteNodes>(b: &mut B, root: Skey, rel: &SPath) -> SdbResult<bool> {
    let Some(key) = resolve_from(&*b, root, rel)? else {
        return Ok(false);
    };

    cascade_delete(b, key)?;

    if let Some((parent_rel, last)) = rel.split_last()
        && let Some(parent_key) = resolve_from(&*b, root, &parent_rel)?
    {
        detach_child(b, parent_key, &parent_rel, last)?;
    }

    Ok(true)
}

/// Ensures a container node exists at `path` and returns its primary key.
pub(crate) fn ensure_container<B: WriteNodes>(b: &mut B, path: &SPath, child_is_index: bool) -> SdbResult<Skey> {
    ensure_container_rel(b, Skey::ROOT, path, child_is_index)
}

/// Ensures a container exists at `rel` relative to `root` (creating intermediate
/// containers as needed) and returns its key. With `rel` empty this ensures
/// `root` itself, creating it when it is the table root and absent.
pub(crate) fn ensure_container_rel<B: WriteNodes>(
    b: &mut B,
    root: Skey,
    rel: &SPath,
    child_is_index: bool,
) -> SdbResult<Skey> {
    let Some((parent_rel, last)) = rel.split_last() else {
        return match read_node(&*b, root)? {
            Some(node) => verify_container(node.kind(), &SPath::root(), child_is_index).map(|()| root),
            None if root == Skey::ROOT => {
                write_node(b, Skey::ROOT, empty_container(child_is_index))?;
                Ok(Skey::ROOT)
            }
            None => Err(SdbError::Corrupt("relative-store root points to a missing node".into())),
        };
    };

    if let Some(key) = resolve_from(&*b, root, rel)? {
        let node = read_node(&*b, key)?.ok_or_else(|| {
            const MSG: &str = "resolved path points to a missing node";

            SdbError::Corrupt(MSG.into())
        })?;

        return verify_container(node.kind(), rel, child_is_index).map(|()| key);
    }

    let parent_key = ensure_container_rel(b, root, &parent_rel, matches!(last, Segment::Index(_)))?;

    let key = Skey::generate();
    write_node(b, key, empty_container(child_is_index))?;
    attach_child(b, parent_key, &parent_rel, last, key)?;
    Ok(key)
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
fn attach_child<B: WriteNodes>(
    b: &mut B,
    parent_key: Skey,
    parent_path: &SPath,
    last: &Segment,
    child: Skey,
) -> SdbResult<()> {
    match last {
        // An object link is a single point write; the parent node is never read or
        // rewritten, so a wide object no longer costs O(siblings) per child.
        Segment::Name(name) => put_object_child(b, parent_key, name, child),
        Segment::Index(index) => {
            let mut node = read_node(&*b, parent_key)?.ok_or_else(|| {
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
                Node::Packed {
                    ..
                } => return Err(unexpected(parent_path, "list", "packed entity")),
            }

            write_node(b, parent_key, node)
        }
    }
}

/// Unlinks the final segment from `parent`. An object child drops its child-link
/// entry; a list element shifts the following elements left in the vector — and
/// because paths are walked (not indexed), nothing else needs rewriting.
fn detach_child<B: WriteNodes>(b: &mut B, parent_key: Skey, parent_path: &SPath, last: &Segment) -> SdbResult<()> {
    match last {
        Segment::Name(name) => remove_object_child(b, parent_key, name),
        Segment::Index(index) => {
            let mut node = read_node(&*b, parent_key)?.ok_or_else(|| {
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
                Node::Packed {
                    ..
                } => return Err(unexpected(parent_path, "list", "packed entity")),
            }

            write_node(b, parent_key, node)
        }
    }
}

/// Reorders a list element from `from` to `to` within the list node `list_key`.
/// Only the list node's vector changes; the moved subtree keeps its key.
pub(crate) fn list_move<B: WriteNodes>(b: &mut B, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
    let mut node = read_node(&*b, list_key)?.ok_or_else(|| {
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

    write_node(b, list_key, node)
}

/// Swaps the elements at `i` and `j` within the list node `list_key`. Only the
/// list node's vector changes; the swapped subtrees keep their keys.
pub(crate) fn list_swap<B: WriteNodes>(b: &mut B, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
    let mut node = read_node(&*b, list_key)?.ok_or_else(|| {
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

    write_node(b, list_key, node)
}

/// Removes every child of the container node `key` (cascading), leaving an empty
/// container of the same kind. Errors if `key` is a leaf or a packed entity.
pub(crate) fn clear_children<B: WriteNodes>(b: &mut B, key: Skey) -> SdbResult<()> {
    let node = read_node(&*b, key)?.ok_or_else(|| {
        const MSG: &str = "missing node while clearing children";

        SdbError::Corrupt(MSG.into())
    })?;

    match node {
        Node::Object => {
            for child in take_object_children(b, key)? {
                cascade_delete(b, child)?;
            }

            // The object marker stays in place; only its child links are gone.
        }
        Node::List(items) => {
            for child in items {
                cascade_delete(b, child)?;
            }

            write_node(b, key, Node::List(Vec::new()))?;
        }
        Node::Leaf(_) => {
            return Err(SdbError::Corrupt(format!("node {key} is a leaf, expected a container")));
        }
        Node::Packed {
            ..
        } => {
            return Err(SdbError::Corrupt(format!(
                "node {key} is a packed entity, expected a container"
            )));
        }
    }

    Ok(())
}

/// Deletes the subtree rooted at `key` (its node entry and all descendants'). A
/// packed entity is one entry holding its whole subtree, so deleting it is a
/// single removal — no descent.
fn cascade_delete<B: WriteNodes>(b: &mut B, key: Skey) -> SdbResult<()> {
    let mut stack = vec![key];
    while let Some(key) = stack.pop() {
        if let Some(node) = read_node(&*b, key)? {
            match node {
                // Detaches and collects the object's child links as we descend, so
                // no orphan link entry survives the deletion.
                Node::Object => stack.extend(take_object_children(b, key)?),
                Node::List(items) => stack.extend(items),
                Node::Leaf(_)
                | Node::Packed {
                    ..
                } => {}
            }
        }

        delete_node(b, key)?;
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

// --------------------------------------------------------------------------
// Packed entities
// --------------------------------------------------------------------------

/// Writes the packed-entity `node` at `parent_path / last`, ensuring the parent
/// container exists and linking the entity (a fresh key) into it.
pub(crate) fn store_packed<B: WriteNodes>(b: &mut B, parent_path: &SPath, last: &Segment, node: Node) -> SdbResult<()> {
    let parent_key = ensure_container(b, parent_path, matches!(last, Segment::Index(_)))?;
    let entity = Skey::generate();
    write_node(b, entity, node)?;
    attach_child(b, parent_key, parent_path, last, entity)
}

/// Decodes the packed entity at `key` into its in-memory mini node-table.
pub(crate) fn decode_packed<B: ReadNodes>(b: &B, key: Skey) -> SdbResult<MemNodes> {
    match read_node(b, key)? {
        Some(Node::Packed {
            blob, ..
        }) => MemNodes::from_blob(&blob),
        _ => Err(SdbError::Corrupt("expected a packed entity".into())),
    }
}

/// Overwrites the node at `key` in place (used to write a packed entity's blob,
/// freshly built or edited, without re-linking it into its parent).
pub(crate) fn write_packed<B: WriteNodes>(b: &mut B, key: Skey, node: Node) -> SdbResult<()> {
    write_node(b, key, node)
}

/// Spills the packed entity at `entity` back into the live store as a plain
/// shredded subtree, replacing its single packed value. The subtree's own keys
/// are preserved; only the blob's internal root (`Skey::ROOT`) is re-mapped to
/// `entity`, so the parent's existing link still points at it. Index entries are
/// unaffected (same entity key, same column values), so no re-indexing is needed.
pub(crate) fn unpack_entity<B: WriteNodes>(b: &mut B, entity: Skey) -> SdbResult<()> {
    let mem = decode_packed(b, entity)?;

    for (key, value) in mem.into_entries() {
        let key = match key {
            TableKey::Data(k) if k == Skey::ROOT => TableKey::Data(entity),
            TableKey::Child {
                parent,
                name,
            } if parent == Skey::ROOT => TableKey::Child {
                parent: entity,
                name,
            },
            other => other,
        };

        b.put(key, value)?;
    }

    Ok(())
}

/// Reads the scalar of the leaf at `rel` relative to `entity`, transparently
/// descending into a packed entity's blob. Returns `None` when the path is absent
/// or does not land on a leaf — exactly the `Null`-column contract index
/// maintenance relies on.
pub(crate) fn entity_leaf<B: ReadNodes>(b: &B, entity: Skey, rel: &SPath) -> SdbResult<Option<Scalar>> {
    match read_node(b, entity)? {
        Some(Node::Packed {
            ..
        }) => {
            let mem = decode_packed(b, entity)?;
            match resolve_from(&mem, Skey::ROOT, rel)? {
                Some(key) => leaf_scalar_opt(&mem, key),
                None => Ok(None),
            }
        }
        _ => match resolve_from(b, entity, rel)? {
            Some(key) => leaf_scalar_opt(b, key),
            None => Ok(None),
        },
    }
}
