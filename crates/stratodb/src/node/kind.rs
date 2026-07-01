/// The kind of a node, as reported by the public API.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// A map from field names to child nodes.
    Object,
    /// A zero-based sequence of child nodes.
    List,
    /// A single scalar value.
    Leaf,
}

impl NodeKind {
    /// A short, stable label used in diagnostics.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            NodeKind::Object => "object",
            NodeKind::List => "list",
            NodeKind::Leaf => "leaf",
        }
    }

    /// A one-byte tag, used to record a packed entity's root kind.
    pub(crate) fn as_tag(self) -> u8 {
        match self {
            NodeKind::Object => 0,
            NodeKind::List => 1,
            NodeKind::Leaf => 2,
        }
    }

    /// Rebuilds a kind from [`as_tag`](Self::as_tag).
    pub(crate) fn from_tag(tag: u8) -> crate::error::SdbResult<NodeKind> {
        match tag {
            0 => Ok(NodeKind::Object),
            1 => Ok(NodeKind::List),
            2 => Ok(NodeKind::Leaf),
            other => Err(crate::error::SdbError::Corrupt(format!(
                "unknown node-kind tag {other}"
            ))),
        }
    }
}
