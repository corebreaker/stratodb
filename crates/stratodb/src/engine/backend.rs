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

use super::{
    archived::{self, ArchNode, ArchTree, ArchivedNodes, NodeView},
    TableKey,
    TableValue,
};
use crate::{
    data::Scalar,
    error::{SdbError, SdbResult},
    node::Node,
    node::NodeKind,
    Skey,
};

use redb::{ReadableTable, Table};
use std::collections::{BTreeMap, HashMap};

/// An iterator over engine entries from a lower bound, in ascending key order.
pub(crate) type NodeIter<'a> = Box<dyn Iterator<Item = SdbResult<(TableKey, TableValue)>> + 'a>;

/// Read access to a node store: point lookup plus an ordered forward scan.
pub(crate) trait ReadNodes {
    fn fetch(&self, key: &TableKey) -> SdbResult<Option<TableValue>>;

    /// Entries at or after `lower`, in ascending key order. Callers stop as soon as
    /// a key falls outside the range they care about (e.g. a child block).
    fn scan_from(&self, lower: &TableKey) -> SdbResult<NodeIter<'_>>;

    /// The child key under object `parent` for field `name`. The default builds the
    /// `Child` key and fetches it; the in-memory backend overrides this to look up
    /// by borrowed name, avoiding a `String` allocation on every navigation hop
    /// (the read/recompose hot path runs entirely over that backend).
    fn child_link(&self, parent: Skey, name: &str) -> SdbResult<Option<Skey>> {
        let key = TableKey::Child {
            parent,
            name: name.to_string(),
        };

        match self.fetch(&key)? {
            Some(TableValue::Skey(child)) => Ok(Some(child)),
            Some(_) => Err(crate::error::SdbError::Corrupt("object child link is not a key".into())),
            None => Ok(None),
        }
    }
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

/// A node held in the in-memory store. An object keeps its children **inline**
/// (a name-sorted map), so a child lookup is a direct map access with no key
/// allocation — unlike the engine table, where object children are separate
/// `Child` entries. A list and a leaf mirror the engine node.
#[derive(Clone, Debug)]
enum MemNode {
    Object(BTreeMap<String, Skey>),
    List(Vec<Skey>),
    Leaf(Scalar),
}

impl MemNode {
    fn kind(&self) -> NodeKind {
        match self {
            MemNode::Object(_) => NodeKind::Object,
            MemNode::List(_) => NodeKind::List,
            MemNode::Leaf(_) => NodeKind::Leaf,
        }
    }

    /// The engine [`Node`] this in-memory node presents (an object is a marker —
    /// its children live in `Child` entries at the engine boundary).
    fn engine_node(&self) -> Node {
        match self {
            MemNode::Object(_) => Node::Object,
            MemNode::List(items) => Node::List(items.clone()),
            MemNode::Leaf(scalar) => Node::Leaf(scalar.clone()),
        }
    }
}

/// An in-memory mini node-table backing a packed entity. Objects store children
/// inline for allocation-free navigation; the **on-disk blob is unchanged** — it
/// is still the engine's `(key, value)` entry list (see [`to_blob`](Self::to_blob)
/// / [`from_blob`](Self::from_blob)), just assembled into this faster shape.
#[derive(Clone, Debug, Default)]
pub(crate) struct MemNodes {
    nodes: HashMap<Skey, MemNode>,
}

impl MemNodes {
    /// An empty store (its root node is created by the first write).
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The kind of the subtree root (node [`Skey::ROOT`]). A never-written entity
    /// (e.g. an empty struct) packs as an empty object.
    pub(crate) fn root_kind(&self) -> SdbResult<NodeKind> {
        Ok(self
            .nodes
            .get(&Skey::ROOT)
            .map(MemNode::kind)
            .unwrap_or(NodeKind::Object))
    }

    /// Serializes the store into a packed-entity blob: an rkyv-archived
    /// [`ArchTree`] of `(key, node)` pairs sorted by key (children reference one
    /// another by key, preserving node identity). Leaf scalars keep the byte codec.
    pub(crate) fn to_blob(&self) -> SdbResult<Vec<u8>> {
        let mut nodes: Vec<([u8; 16], ArchNode)> = self
            .nodes
            .iter()
            .map(|(key, node)| {
                let arch = match node {
                    // `children` is a `BTreeMap`, so the pairs come out name-sorted —
                    // exactly what the read-side binary search over names relies on.
                    MemNode::Object(children) => ArchNode::Object(
                        children
                            .iter()
                            .map(|(name, child)| (name.clone(), child.into_bytes()))
                            .collect(),
                    ),
                    MemNode::List(items) => ArchNode::List(items.iter().map(|child| child.into_bytes()).collect()),
                    MemNode::Leaf(scalar) => {
                        let mut bytes = Vec::new();
                        scalar.encode(&mut bytes);
                        ArchNode::Leaf(bytes)
                    }
                };

                (key.into_bytes(), arch)
            })
            .collect();

        // A never-written entity still archives a (root) empty object.
        if nodes.is_empty() {
            nodes.push((Skey::ROOT.into_bytes(), ArchNode::Object(Vec::new())));
        }

        // Sorted by key so the read side can binary-search nodes.
        nodes.sort_by_key(|(key, _)| *key);

        archived::to_bytes(&ArchTree {
            nodes,
        })
    }

    /// Rebuilds a mutable store from a blob (for in-place edits / unpacking),
    /// preserving every node's key.
    pub(crate) fn from_blob(blob: &[u8]) -> SdbResult<MemNodes> {
        let arch = ArchivedNodes::new(blob)?;

        let mut nodes = HashMap::new();
        for (key, view) in arch.entries()? {
            let node = match view {
                NodeView::Object(pairs) => MemNode::Object(pairs.into_iter().collect()),
                NodeView::List(items) => MemNode::List(items),
                NodeView::Leaf(scalar) => MemNode::Leaf(scalar),
            };

            nodes.insert(key, node);
        }

        Ok(MemNodes {
            nodes,
        })
    }

    /// Folds one decoded engine `(key, value)` entry into the inline rep.
    fn apply(&mut self, key: TableKey, value: TableValue) -> SdbResult<()> {
        match key {
            TableKey::Data(node_key) => {
                let node = match value {
                    TableValue::Node(Node::Object) => MemNode::Object(BTreeMap::new()),
                    TableValue::Node(Node::List(items)) => MemNode::List(items),
                    TableValue::Node(Node::Leaf(scalar)) => MemNode::Leaf(scalar),
                    TableValue::Node(Node::Packed {
                        ..
                    }) => {
                        return Err(SdbError::Corrupt("nested packed entity in a blob".into()));
                    }
                    _ => return Err(SdbError::Corrupt("data entry is not a node".into())),
                };

                // Re-applying an object marker keeps any children already folded in.
                if matches!(
                    (self.nodes.get(&node_key), &node),
                    (Some(MemNode::Object(_)), MemNode::Object(_))
                ) {
                    return Ok(());
                }

                self.nodes.insert(node_key, node);
            }
            TableKey::Child {
                parent,
                name,
            } => {
                let TableValue::Skey(child) = value else {
                    return Err(SdbError::Corrupt("object child link is not a key".into()));
                };

                match self
                    .nodes
                    .entry(parent)
                    .or_insert_with(|| MemNode::Object(BTreeMap::new()))
                {
                    MemNode::Object(children) => {
                        children.insert(name, child);
                    }
                    _ => return Err(SdbError::Corrupt("child link parent is not an object".into())),
                }
            }
            TableKey::Index {
                ..
            } => return Err(SdbError::Corrupt("index entry in a blob".into())),
        }

        Ok(())
    }

    /// Packs this store into a [`Node::Packed`] carrying its root kind and blob.
    pub(crate) fn into_packed(self) -> SdbResult<Node> {
        Ok(Node::Packed {
            root: self.root_kind()?,
            blob: self.to_blob()?,
        })
    }

    /// Consumes the store, yielding its engine `(key, value)` entries. Used to
    /// spill a packed entity back into the live table (unpacking).
    pub(crate) fn into_entries(self) -> impl Iterator<Item = (TableKey, TableValue)> {
        let mut out = Vec::new();

        for (key, node) in self.nodes {
            match node {
                MemNode::Object(children) => {
                    out.push((TableKey::Data(key), TableValue::Node(Node::Object)));
                    for (name, child) in children {
                        out.push((
                            TableKey::Child {
                                parent: key,
                                name,
                            },
                            TableValue::Skey(child),
                        ));
                    }
                }
                MemNode::List(items) => out.push((TableKey::Data(key), TableValue::Node(Node::List(items)))),
                MemNode::Leaf(scalar) => out.push((TableKey::Data(key), TableValue::Node(Node::Leaf(scalar)))),
            }
        }

        out.into_iter()
    }
}

impl ReadNodes for MemNodes {
    fn fetch(&self, key: &TableKey) -> SdbResult<Option<TableValue>> {
        match key {
            TableKey::Data(node_key) => Ok(self
                .nodes
                .get(node_key)
                .map(|node| TableValue::Node(node.engine_node()))),
            TableKey::Child {
                parent,
                name,
            } => Ok(match self.nodes.get(parent) {
                Some(MemNode::Object(children)) => children.get(name).map(|child| TableValue::Skey(*child)),
                _ => None,
            }),
            TableKey::Index {
                ..
            } => Ok(None),
        }
    }

    fn scan_from(&self, lower: &TableKey) -> SdbResult<NodeIter<'_>> {
        // The tree only scans a parent's `Child` block (object children); other
        // bounds never occur here.
        let TableKey::Child {
            parent,
            name,
        } = lower
        else {
            return Ok(Box::new(std::iter::empty()));
        };

        match self.nodes.get(parent) {
            Some(MemNode::Object(children)) => {
                let parent = *parent;

                Ok(Box::new(children.range(name.clone()..).map(move |(name, child)| {
                    Ok((
                        TableKey::Child {
                            parent,
                            name: name.clone(),
                        },
                        TableValue::Skey(*child),
                    ))
                })))
            }
            _ => Ok(Box::new(std::iter::empty())),
        }
    }

    fn child_link(&self, parent: Skey, name: &str) -> SdbResult<Option<Skey>> {
        Ok(match self.nodes.get(&parent) {
            Some(MemNode::Object(children)) => children.get(name).copied(),
            _ => None,
        })
    }
}

impl WriteNodes for MemNodes {
    fn put(&mut self, key: TableKey, value: TableValue) -> SdbResult<()> {
        self.apply(key, value)
    }

    fn delete(&mut self, key: &TableKey) -> SdbResult<()> {
        match key {
            TableKey::Data(node_key) => {
                self.nodes.remove(node_key);
            }
            TableKey::Child {
                parent,
                name,
            } => {
                if let Some(MemNode::Object(children)) = self.nodes.get_mut(parent) {
                    children.remove(name);
                }
            }
            TableKey::Index {
                ..
            } => {}
        }

        Ok(())
    }
}
