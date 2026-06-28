use crate::{
    data::Scalar,
    path::{IntoPath, SPath, Segment},
};

use std::{collections::BTreeMap, mem::replace};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Leaf(Scalar),
    List(Vec<Value>),
    Node(BTreeMap<String, Value>),
}

impl Value {
    pub fn new_leaf(value: Scalar) -> Self {
        Value::Leaf(value)
    }

    pub fn new_empty_list() -> Self {
        Value::List(Vec::new())
    }

    pub fn new_empty_node() -> Self {
        Value::Node(BTreeMap::new())
    }

    pub fn new_list(list: Vec<Value>) -> Self {
        Value::List(list)
    }

    pub fn new_node(node: BTreeMap<String, Value>) -> Self {
        Value::Node(node)
    }

    pub fn leaf(&self) -> Option<&Scalar> {
        match self {
            Self::Leaf(leaf) => Some(leaf),
            _ => None,
        }
    }

    pub fn leaf_mut(&mut self) -> Option<&mut Scalar> {
        match self {
            Self::Leaf(leaf) => Some(leaf),
            _ => None,
        }
    }

    pub fn list(&self) -> Option<&[Value]> {
        match self {
            Self::List(list) => Some(list),
            _ => None,
        }
    }

    pub fn list_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::List(list) => Some(list),
            _ => None,
        }
    }

    pub fn node(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Node(node) => Some(node),
            _ => None,
        }
    }

    pub fn node_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Self::Node(node) => Some(node),
            _ => None,
        }
    }

    pub fn at(&self, index: usize) -> Option<&Value> {
        match self {
            Self::List(list) => list.get(index),
            _ => None,
        }
    }

    pub fn at_mut(&mut self, index: usize) -> Option<&mut Value> {
        match self {
            Self::List(list) => list.get_mut(index),
            _ => None,
        }
    }

    pub fn contains_key(&self, key: impl AsRef<str>) -> bool {
        if let Self::Node(node) = self {
            node.contains_key(key.as_ref())
        } else {
            false
        }
    }

    pub fn get(&self, key: impl AsRef<str>) -> Option<&Value> {
        match self {
            Self::Node(node) => node.get(key.as_ref()),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: impl AsRef<str>) -> Option<&mut Value> {
        match self {
            Self::Node(node) => node.get_mut(key.as_ref()),
            _ => None,
        }
    }

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

    pub fn push(&mut self, value: Value) {
        if let Self::List(list) = self {
            list.push(value);
        }
    }

    pub fn insert(&mut self, key: String, value: Value) {
        if let Self::Node(node) = self {
            node.insert(key, value);
        }
    }

    pub fn remove_at(&mut self, index: usize) {
        if let Self::List(list) = self {
            list.remove(index);
        }
    }

    pub fn remove_key(&mut self, key: impl AsRef<str>) {
        if let Self::Node(node) = self {
            node.remove(key.as_ref());
        }
    }

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
    fn default() -> Self {
        Value::Leaf(Scalar::Null)
    }
}
