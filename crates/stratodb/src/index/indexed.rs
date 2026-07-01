//! [`SIndexed`]: the secondary indexes a type declares.

use super::IndexDef;

/// A type that declares secondary indexes, via `#[derive(SData)]`'s
/// `#[strato(index(...))]` attributes.
///
/// The attributes fix each index's **schema** — its name, columns (in priority
/// order, each ASC or DESC) and uniqueness. [`index_defs`](SIndexed::index_defs)
/// pairs that schema with a **scope** (a path `pattern`) to produce ready-to-create
/// [`IndexDef`]s; [`Table::create_indexes`](crate::Table::create_indexes) registers
/// them in one call. The two compose: the type says *what* to index, the call says
/// *which* entities.
pub trait SIndexed {
    /// The indexes declared on this type, each scoped to `pattern`.
    ///
    /// `pattern` is a slash-separated **path pattern** naming which nodes in the
    /// tree are the indexed *entities* — the records the index sorts, and the
    /// anchor each column path is resolved against. A `*` segment is a wildcard
    /// matching any single child; every other segment matches literally (a list
    /// index such as `items[0]` is allowed). It is **not** recursive: `*` spans
    /// exactly one level.
    ///
    /// For example `"users/*"` makes every direct child of `users`
    /// (`users/alice`, `users/bob`, …) an entity, so a column `age` indexes each
    /// `users/<id>/age`. The empty string `""` scopes the index to the table root
    /// itself (one entity). The same `pattern` scopes every index the type
    /// declares.
    ///
    /// The **schema** (names, columns, uniqueness) is fixed by the type's
    /// `#[strato(index(...))]` attributes; `pattern` is the one thing supplied at
    /// call time — the type says *what* to index, the caller says *which* nodes.
    fn index_defs(pattern: &str) -> Vec<IndexDef>;
}
