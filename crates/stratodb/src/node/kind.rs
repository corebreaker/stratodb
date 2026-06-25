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
}
