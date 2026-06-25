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
    fn index_defs(pattern: &str) -> Vec<IndexDef>;
}
