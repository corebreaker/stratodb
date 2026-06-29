//! The dynamic, in-memory document type, [`Value`].
//!
//! `Value` mirrors the stored node tree — `Leaf(Scalar)` / `List` / `Node` —
//! and is *faithful*: each leaf keeps its exact [`Scalar`], so it round-trips
//! losslessly through [`load_value`](crate::txn::ReadTxn::load_value) /
//! [`store_value`](crate::txn::WriteTxn::store_value). It is the one dynamic
//! value type; textual export projects each leaf at render time rather than
//! through a parallel type.
//!
//! Beyond the in-memory accessors (`leaf` / `list` / `node`, `get` / `at`,
//! `push` / `insert` / `merge`, …) it carries path-addressed
//! [`get_value`](Value::get_value) / [`set_value`](Value::set_value), which
//! create containers as needed and never silently destroy data along the way.

use crate::{
    data::Scalar,
    path::{IntoPath, SPath, Segment},
};

use std::{collections::BTreeMap, mem::replace};

/// A dynamic document: a scalar leaf, an ordered list, or a named map of
/// values — the faithful in-memory mirror of the stored node tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A single scalar value.
    Leaf(Scalar),
    /// An ordered sequence of values, addressable by index.
    List(Vec<Value>),
    /// A map of named values, kept in sorted key order.
    Node(BTreeMap<String, Value>),
}

impl Value {
    /// A leaf holding `value`.
    pub fn new_leaf(value: Scalar) -> Self {
        Value::Leaf(value)
    }

    /// An empty list.
    pub fn new_empty_list() -> Self {
        Value::List(Vec::new())
    }

    /// An empty node (named map).
    pub fn new_empty_node() -> Self {
        Value::Node(BTreeMap::new())
    }

    /// A list of the given values.
    pub fn new_list(list: Vec<Value>) -> Self {
        Value::List(list)
    }

    /// A node wrapping the given map.
    pub fn new_node(node: BTreeMap<String, Value>) -> Self {
        Value::Node(node)
    }

    /// The scalar, if this is a [`Leaf`](Value::Leaf).
    pub fn leaf(&self) -> Option<&Scalar> {
        match self {
            Self::Leaf(leaf) => Some(leaf),
            _ => None,
        }
    }

    /// A mutable reference to the scalar, if this is a [`Leaf`](Value::Leaf).
    pub fn leaf_mut(&mut self) -> Option<&mut Scalar> {
        match self {
            Self::Leaf(leaf) => Some(leaf),
            _ => None,
        }
    }

    /// The elements, if this is a [`List`](Value::List).
    pub fn list(&self) -> Option<&[Value]> {
        match self {
            Self::List(list) => Some(list),
            _ => None,
        }
    }

    /// A mutable reference to the element vector, if this is a [`List`](Value::List).
    pub fn list_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::List(list) => Some(list),
            _ => None,
        }
    }

    /// The entries, if this is a [`Node`](Value::Node).
    pub fn node(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Node(node) => Some(node),
            _ => None,
        }
    }

    /// A mutable reference to the entry map, if this is a [`Node`](Value::Node).
    pub fn node_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Self::Node(node) => Some(node),
            _ => None,
        }
    }

    /// The element at `index`, if this is a list reaching that far.
    pub fn at(&self, index: usize) -> Option<&Value> {
        match self {
            Self::List(list) => list.get(index),
            _ => None,
        }
    }

    /// A mutable reference to the element at `index`, if this is a list reaching
    /// that far.
    pub fn at_mut(&mut self, index: usize) -> Option<&mut Value> {
        match self {
            Self::List(list) => list.get_mut(index),
            _ => None,
        }
    }

    /// Whether this is a node containing `key`. `false` for a leaf or list.
    pub fn contains_key(&self, key: impl AsRef<str>) -> bool {
        if let Self::Node(node) = self {
            node.contains_key(key.as_ref())
        } else {
            false
        }
    }

    /// The value under `key`, if this is a node containing it.
    pub fn get(&self, key: impl AsRef<str>) -> Option<&Value> {
        match self {
            Self::Node(node) => node.get(key.as_ref()),
            _ => None,
        }
    }

    /// A mutable reference to the value under `key`, if this is a node containing
    /// it.
    pub fn get_mut(&mut self, key: impl AsRef<str>) -> Option<&mut Value> {
        match self {
            Self::Node(node) => node.get_mut(key.as_ref()),
            _ => None,
        }
    }

    /// Empties the value in place: a leaf becomes [`Null`](Scalar::Null), a list
    /// or node drops its children (keeping its kind).
    pub fn clear(&mut self) {
        match self {
            Self::Leaf(scalar) => {
                *scalar = Scalar::Null;
            }
            Self::List(list) => {
                list.clear();
            }
            Self::Node(node) => node.clear(),
        }
    }

    /// Appends `value` if this is a list; a no-op on a leaf or node.
    pub fn push(&mut self, value: Value) {
        if let Self::List(list) = self {
            list.push(value);
        }
    }

    /// Inserts (or replaces) the `key` → `value` entry if this is a node; a no-op
    /// on a leaf or list.
    pub fn insert(&mut self, key: String, value: Value) {
        if let Self::Node(node) = self {
            node.insert(key, value);
        }
    }

    /// Removes the element at `index` if this is a list; a no-op on a leaf or
    /// node. Panics if `index` is out of bounds (see [`Vec::remove`]).
    pub fn remove_at(&mut self, index: usize) {
        if let Self::List(list) = self {
            list.remove(index);
        }
    }

    /// Removes the `key` entry if this is a node; a no-op on a leaf or list.
    pub fn remove_key(&mut self, key: impl AsRef<str>) {
        if let Self::Node(node) = self {
            node.remove(key.as_ref());
        }
    }

    /// Merges `with` into `self`, by kind:
    ///
    /// - a [`Leaf`](Value::Leaf) is replaced wholesale by `with`;
    /// - a [`List`](Value::List) extends with another list's elements, or pushes a non-list `with` as one more element;
    /// - a [`Node`](Value::Node) extends with another node (overwriting duplicate keys), or is replaced wholesale by a
    ///   non-node `with`.
    pub fn merge(&mut self, with: Self) {
        match self {
            Self::Leaf(_) => {
                let _ = replace(self, with);
            }
            Self::List(list) => match with {
                Self::List(with_list) => {
                    list.extend(with_list);
                }
                with => {
                    list.push(with);
                }
            },
            Self::Node(node) => match with {
                Self::Node(with_node) => {
                    node.extend(with_node);
                }
                with => {
                    let _ = replace(self, with);
                }
            },
        }
    }

    /// Returns a clone of the subtree at `path`, or `None` if no node sits there
    /// (or `path` does not parse). The root path returns the whole value.
    pub fn get_value(&self, path: impl IntoPath) -> Option<Value> {
        Some(self.subtree(&path.into_path().ok()?)?.clone())
    }

    /// Sets `value` at `path`, creating missing containers on the way (a `Name`
    /// segment makes an object, an `Index` a list), and returns whether it applied.
    ///
    /// It never descends through or overwrites a leaf sitting *along* the path, and
    /// never grows a list past its end; on such a conflict (or a path that does not
    /// parse) it leaves `self` untouched and returns `false`. The value at the
    /// destination itself is replaced. The root path replaces the whole value.
    pub fn set_value(&mut self, path: impl IntoPath, value: Value) -> bool {
        let Ok(base) = path.into_path() else {
            return false;
        };

        set_in(self, base.segments(), value)
    }

    /// Borrows the subtree at `base`, following each segment (`Name` into objects,
    /// `Index` into lists); `None` if a segment leads nowhere.
    pub(crate) fn subtree(&self, base: &SPath) -> Option<&Value> {
        let mut current = self;
        for segment in base.segments() {
            current = match segment {
                Segment::Name(name) => current.get(name)?,
                Segment::Index(index) => current.at(*index as usize)?,
            };
        }

        Some(current)
    }
}

/// Places `value` at `segments` within `target`: descends into existing
/// containers, creates missing ones, and replaces the destination. Returns
/// `false` without mutating `target` if a segment would traverse a leaf, hit the
/// wrong container kind, or grow a list past its end.
fn set_in(target: &mut Value, segments: &[Segment], value: Value) -> bool {
    let Some((first, rest)) = segments.split_first() else {
        *target = value;

        return true;
    };

    match (target, first) {
        (Value::Node(map), Segment::Name(name)) => match map.get_mut(name) {
            Some(child) => set_in(child, rest, value),
            None => match build_fresh(rest, value) {
                Some(subtree) => {
                    map.insert(name.clone(), subtree);

                    true
                }
                None => false,
            },
        },
        (Value::List(list), Segment::Index(index)) => {
            let index = *index as usize;

            if index < list.len() {
                set_in(&mut list[index], rest, value)
            } else if index == list.len() {
                match build_fresh(rest, value) {
                    Some(subtree) => {
                        list.push(subtree);

                        true
                    }
                    None => false,
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Builds a brand-new subtree placing `value` at `segments`. `None` if it cannot
/// be created — an `Index` in fresh territory can only be `0`, since a new list
/// starts empty, so any higher index has no slot.
fn build_fresh(segments: &[Segment], value: Value) -> Option<Value> {
    let Some((first, rest)) = segments.split_first() else {
        return Some(value);
    };

    match first {
        Segment::Name(name) => {
            let mut map = BTreeMap::new();
            map.insert(name.clone(), build_fresh(rest, value)?);

            Some(Value::Node(map))
        }
        Segment::Index(0) => Some(Value::List(vec![build_fresh(rest, value)?])),
        Segment::Index(_) => None,
    }
}

impl Default for Value {
    /// The empty default is a [`Null`](Scalar::Null) leaf.
    fn default() -> Self {
        Value::Leaf(Scalar::Null)
    }
}
