# CLAUDE.md — StratoDB developer guide

## Language

All communication with the user is in **French**. Code, identifiers, comments, doc-strings, commit messages, and test names stay in **English**.

---

## Project overview

StratoDB is a typed, transactional, indexed document store written in Rust, layered over **redb v4.1.0** (kept fully opaque — no redb type ever surfaces in the public API). Data is fully shredded into a tree of typed nodes (objects, lists, scalar leaves), each bearing an opaque `Skey` primary key. Paths (`SPath`) are ephemeral addresses resolved by walking the tree at query time.

Repository: https://github.com/corebreaker/stratodb  
Working branch: `attributes` (derive-attrs milestone); becomes `main` at release.

---

## Workspace layout

```
stratodb/
├── Cargo.toml                  workspace root (resolver = 3, edition = 2024)
├── .rustfmt.toml               nightly-only fmt config (see Style section)
├── crates/
│   ├── stratodb/               runtime crate
│   │   ├── src/
│   │   ├── tests/
│   │   └── Cargo.toml
│   ├── stratodb-derive/        proc-macro crate (#[derive(SData)])
│   │   └── src/
│   └── examples/               runnable examples (not a Cargo member — excluded via workspace.exclude)
│       ├── basic.rs
│       └── indexed.rs
```

`crates/examples/` has no `Cargo.toml`; examples are declared as `[[example]]` targets in `crates/stratodb/Cargo.toml`. The workspace `exclude = ["crates/examples"]` prevents Cargo from treating the directory as a broken member.

---

## Build and test — the full gate

Run these before every commit. All four must be green:

```sh
# tests (lib + integration)
cargo test --all-features --all-targets

# doctests (--all-targets skips them)
cargo test --all-features --doc

# lint — default features
cargo clippy --all-targets -- -D warnings

# lint — with derive feature
cargo clippy --all-targets --features derive -- -D warnings

# formatter (nightly ONLY — stable fmt silently ignores the config)
cargo +nightly fmt --check
```

`tests/derive.rs` and `tests/index_typed.rs` are gated with `#![cfg(feature = "derive")]`; include `--features derive` (or `--all-features`) to run them.

Examples:

```sh
cargo run -p stratodb --example basic
cargo run -p stratodb --example indexed --features derive
```

---

## Code style

The style is **hand-formatted and airy**; `cargo +nightly fmt` is the only formatter (never stable `cargo fmt` — it would silently undo alignment and struct-literal expansion).

Key rules that rustfmt alone does not enforce:

- **Field alignment:** in struct/enum definitions, struct-variant literals, and constructor calls with ≥ 2 named fields, pad names so types/values form a column (`path:     SPath`, `expected: &'static str`).
- **Statement groups:** separate logical groups with a blank line — after a guard/early-return, before a trailing `Ok(…)` or final expression inside a long match arm, between a "header" block and a following loop.
- **Struct literals always expanded** (one field per line), even when short.
- **Import order:** `use crate::…` block first, then a blank line, then third-party/std. Order within a block is semantic, not alphabetical.
- **One concept per file.** Large modules become a directory + `mod.rs` that only declares sub-mods and re-exports (`pub(crate) use self::…`).
- **Doc-comments:** `//!` on every file; `///` on every public item and every enum/struct field.
- **Comments in code:** only when the *why* is non-obvious. No narration of what the code does.
- **Error type:** `SdbError` / `SdbResult<T>` everywhere (never `anyhow`, never raw `Box<dyn Error>`).

---

## Commit messages

Each paragraph is **one physical line**; blank lines separate paragraphs. No column wrapping (no 72-char margin). Subject line is the first paragraph; optional body follows after a blank line.

```
Short imperative subject

Longer explanation if needed, written as a single continuous paragraph even
if it is many words long — do NOT insert manual line breaks within it.
```

---

## Architecture — locked decisions

These choices are final and must not be revisited or worked around:

**Storage model — full shredding.** Every scalar value is its own node with its own `Skey`. Paths are never persisted; they resolve by walking the tree from the fixed root key (`Skey::ROOT` = nil UUID). One redb file per `StratoDb`; one engine table per StratoDB table holds both data nodes and index entries (partitioned by a leading discriminant byte). A global reserved `$metadata` table holds the index registry and the format version.

**Node types:** `Object(BTreeMap<name, Skey>)`, `List(Vec<Skey>)`, `Leaf(Scalar)`. Identity is `Skey` (stable through moves and renames).

**Per-table LRU path cache.** A `PathCache` (256 k entries, `lru` crate) keyed by `(generation, SPath)` is shared across read transactions on the same table. A `version_lock` + atomic generation counter ensure a snapshot never borrows a resolution from another committed version. `WriteTxn` never uses the cache (it sees uncommitted state); `ReadCursor` is cache-backed, `WriteCursor` uses raw walks.

**Engine (redb) is opaque.** No redb type in the public API. No `pub use redb::…` anywhere.

**Index model.** Secondary indexes are named, composite (ordered columns), per-column ASC/DESC, with optional uniqueness. Non-unique: entity in the key (`INDEX_DUP` tag). Unique: entity in the value (`INDEX_UNIQUE` tag); collision = `UniqueViolation`. Scope = path pattern (e.g., `"users/*"`); `*` is a one-segment wildcard. Order-preserving encoding: DESC = bitwise complement; strings use a two-byte `0x00 0x01` terminator (escape `0x00` → `0x00 0xFF`) so encodings are prefix-free.

**Write-time index maintenance.** Every mutation goes through `WriteTxn::reindex_around(scope, apply)`: delete affected index entries, apply the mutation, re-insert. The index set is loaded once per transaction into an `OnceLock<Vec<IndexEntry>>` (not `OnceCell` — keeps `WriteTxn: Sync` so `Arc<WriteCursor>` in `fetch_mut` passes clippy `arc_with_non_send_sync`).

**Accessor GATs.** `SData::Ref<'t>: SRef<'t>`, `SData::Mut<'t>: SMut<'t>`. Keys are **eager and infallible** — resolved at accessor construction; reading a scalar is `acc.x()?.get()?`. Accessors hold `Arc<dyn Reader/Writer + 't>` (type-erased, cheap to clone).

**`derive` feature.** `stratodb-derive` is an optional dependency enabled by the `derive` feature (`default = []`). `#[cfg(feature = "derive")] pub use stratodb_derive::SData;` re-exports the macro next to the trait — one import brings both into scope (Serde-style).

**Defaults.** `Vec<T>` → `List` of nodes (each element addressable). `Option<None>` → `Leaf(Null)` present. `BTreeMap<String, T>` → `Object`. `Bytes` newtype → single leaf (vs `Vec<u8>` which shreds). `store` = replace subtree + auto-create ancestors. `remove` = cascade. Enum = externally tagged `Object { VariantName: payload }`.

---

## Module map — stratodb

```
src/
├── lib.rs                  root; curated re-exports + pub mods
├── constants.rs
├── datetime.rs
├── cache.rs                PathCache (LRU, per-table, shared across read txns)
├── db/                     StratoDb (database.rs); DbInner (inner.rs: generation, version_lock, caches)
├── key.rs                  Skey (opaque 16-byte UUIDv7 primary key)
├── node/                   NodeKind (kind.rs) + Node Object/List/Leaf + encoding (definition.rs)
├── table.rs                Table handle → read()/write()/create_index()
├── tree.rs                 tree walk, node resolution, list helpers
├── codec/                  byte encoding (putters, reader)
├── engine/                 redb table defs, META_TABLE, TableKey/TableValue encoding
├── access/
│   ├── reader.rs           ReadCursor + Reader trait (get_node, child_cached, object_keys…)
│   ├── writer.rs           WriteCursor + Writer trait (put_node, ensure_container…)
│   └── rooted.rs           Rooted<R> adapter (re-roots SData::load at an entity key)
├── data/
│   ├── definition.rs       SData trait (store/load)
│   ├── value.rs            SValue trait + macro_rules for scalar impls
│   ├── scalar.rs           Scalar enum (21 variants)
│   ├── sref.rs / smut.rs   SRef / SMut bound traits
│   ├── leaf_ref.rs / leaf_mut.rs  Leaf<'t,T> / LeafMut<'t,T>
│   ├── seq.rs              Vec<T> → Seq / SeqMut
│   ├── map.rs              BTreeMap<String,T> → Map / MapMut
│   ├── opt.rs              Option<T> → OptRef / OptMut
│   ├── bytes.rs            Bytes newtype
│   └── identifiable.rs     SIdentifiable (key + path from an accessor)
├── path/
│   ├── spath.rs            SPath (immutable slash-separated path; parse normalises ./.. )
│   ├── segment.rs          Segment (field name or list index)
│   ├── functions.rs
│   ├── into_path.rs        IntoPath trait — path args accept &str/String/SPath (parsed or used as-is)
│   └── tail.rs             PathTail trait + / and /= operators on SPath
├── index/
│   ├── definitions/        IndexDef, IndexColumn, Direction
│   ├── registry/           $metadata registry (create/lookup/list by table)
│   ├── id.rs               IndexId
│   ├── indexed.rs          SIndexed trait
│   ├── ordered.rs          order-preserving Scalar codec
│   ├── pattern.rs          Pattern (*-wildcard) + affected_entities(scope)
│   └── maintenance.rs      delete + insert (bracket every mutation)
└── txn/
    ├── read.rs             ReadTxn (get/kind/fetch/load/find/query/rooted)
    ├── write.rs            WriteTxn (put/store/remove/commit/rooted + reindex_around)
    ├── query.rs            IndexQuery builder (.prefixed/.reversed/.under/.run)
    └── rooted/
        ├── read.rs         RootedRead<'a> (borrows ReadTxn, relative paths)
        └── write.rs        RootedWrite<'a> (borrows WriteTxn, relative paths)
```

**Public re-exports** (top-level `use stratodb::…`):
`StratoDb`, `Table`, `SData` (trait + derive macro with feature), `Skey`, `NodeKind`, `SdbError`, `SdbResult`.
Plus `pub mod`: `data`, `error`, `index`, `path`, `txn`, `access`, `constants`.

---

## Module map — stratodb-derive

```
src/
├── lib.rs                  proc_macro_derive entry point #[proc_macro_derive(SData, attributes(strato))]
├── expand_macro.rs         dispatch: delegated (from/into/try_from) → convert; enum → enum_data; else struct pipeline
├── convert.rs              from/into/try_from — SData impl stored AS a target type U (no accessors generated)
├── generics.rs             Generics::analyze — propagate generics + bound onto impls/accessors; `Bounds` alias
├── field_parts.rs          FieldParts<'a> { getter, setter, ty, name, attrs }
├── named_fields.rs         extract named fields; reject tuple/unit structs + unions
├── desc.rs                 StratoXxxDesc codegen (TYPE_NAME, FIELDS, VARIANTS)
├── sdata_impl.rs           struct SData impl codegen (store + load)
├── attr/                   #[strato(...)] parsing
│   ├── container.rs        ContainerAttrs (rename_all, index, from/into/try_from, tag/content/untagged, expecting, bound)
│   ├── field.rs            FieldAttrs (rename, alias, skip*, default, store_with/load_with/with, flatten)
│   ├── variant.rs          VariantAttrs (rename, alias, other)
│   ├── rename.rs           RenameRule (8 casings; apply_to_field / apply_to_variant)
│   ├── default.rs          FieldDefault (Trait / Path)
│   └── misc.rs             parse_path_lit / parse_type_lit / join_path / capitalize
├── refs/
│   ├── ref_type.rs         StratoXxx read accessor codegen
│   └── mut_type.rs         StratoXxxMut write accessor codegen
├── enum_data/
│   ├── expand_macro.rs     enum orchestrator (representation + other/expecting)
│   ├── repr.rs             EnumRepr (External/Adjacent/Internal/Untagged) — tag + payload-base fragments
│   ├── variant_parts.rs    VariantParts (resolved tag + aliases + other flag)
│   ├── accessors.rs        generated variant() accessor
│   ├── store_arm.rs        per-variant store branch (+ internal_store_arm)
│   └── load_arm.rs         per-variant load branch (+ internal_load_arm, untagged_arm)
└── index/
    ├── index_attr.rs       IndexAttr (private fields + accessors) — parse #[strato(index(...))]
    ├── indexed_impl.rs     SIndexed impl codegen (index_defs)
    ├── column_spec.rs      column grammar (field [asc|desc])
    └── item.rs             IndexItem (intermediate representation)
```

Generated code is fully `::stratodb::`-qualified (no import assumptions; trait methods called via UFCS).

---

## Test suite

| File | Feature gate | What it covers |
|------|-------------|----------------|
| `tests/foundation.rs` | — | put/get, node kinds, cascade delete, persist/reopen |
| `tests/typed.rs` | — | hand-written SData + accessor contract (reference for derive output) |
| `tests/containers.rs` | — | Vec/Option/BTreeMap/Bytes roundtrips + accessor API |
| `tests/rooted.rs` | — | RootedRead/RootedWrite, relative paths, scoped index queries |
| `tests/indexes.rs` | — | index registry, maintenance, query builder, unique enforcement |
| `tests/derive.rs` | `derive` | #[derive(SData)]: structs/enums + every `#[strato(...)]` attr (rename/skip/default/with, from/into/try_from, enum reps, generics+bound, flatten) |
| `tests/index_typed.rs` | `derive` | end-to-end derived indexes (back-fill, composite prefix, unique, reopen) |

---

## Milestone roadmap

### COMPLETE

| Milestone | Description |
|-----------|-------------|
| 1 | Foundation: StratoDb, Table, ReadTxn, WriteTxn, SPath, Skey, Node, tree walk, path cache |
| 2 | SData trait + accessors: SValue/Scalar, Leaf/LeafMut, Vec/Option/BTreeMap/Bytes containers, #[derive(SData)] for structs and enums, StratoXxxDesc |
| 3 | Secondary indexes: order-preserving codec, IndexDef + registry, maintenance, pattern matching, query builder, unique enforcement, #[strato(index(...))] derive attr, back-fill |
| derive-attrs | Serde-style `#[strato(...)]` attributes — 7 phases (detailed below) |

Milestone 3 extras (same branch): rooted views (`RootedRead`/`RootedWrite`), `SPath` normalization + `/` operator.

### COMPLETE — Derive-attribute parity

Serde-style `#[strato(...)]` attributes on `#[derive(SData)]` (namespace **`strato`**), implemented and tested across seven phases (`tests/derive.rs`). Excluded with no analogue: `borrow`, `getter`, `variant_identifier`, `field_identifier` (`load` returns owned values; the `Ref` accessor IS the zero-copy story).

**Phases (all DONE, one tested commit each):**

| Phase | Attributes |
|-------|-----------|
| 1 | `rename` / `rename_all` (8 Serde casings) / `alias` |
| 2 | `skip` / `skip_store` / `skip_load` / `skip_store_if` / `default` |
| 3 | `store_with` / `load_with` / `with` |
| 4 | `from` / `into` / `try_from` (container-level: the type is stored AS a target `U`, accessors delegate to `U`'s; a failed `try_from` → `SdbError::Conversion`) |
| 5 | enum reps: `tag` (internally) / `tag`+`content` (adjacently) / `untagged` / `other` catch-all; enum `rename_all`, variant `rename`/`alias`; `expecting` |
| 6 | generics + `bound` (single override, not a load/store split — there is one `SData` impl) |
| 7 | `flatten` |

**Key implementation facts:**
- Effective stored name = `rename` > `rename_all(ident)` > the Rust ident; it drives store/load, accessor child-navigation and `Desc::FIELDS`. Getter method names stay the Rust idents.
- Parsing lives in an `attr/` module: `ContainerAttrs` (type-level), `FieldAttrs` (field), `VariantAttrs` (enum variant), `RenameRule` (`apply_to_field` for snake_case fields, `apply_to_variant` for PascalCase variants); the `index(...)` attr folds in there too.
- `store_with`/`load_with`/`with` swap the single `SData::store`/`load` call site (signatures mirror `SData` with the value passed explicitly); they compose with `rename`/`alias`/`default`/`skip_store_if`.
- `from`/`into`/`try_from` route through `convert.rs` (no `StratoXxx`/`Desc` generated, so newtype/tuple structs AND enums are accepted on that path).
- Enum representations: `enum_data/repr.rs` `EnumRepr`. Internally tagged keys a tuple/newtype payload by decimal index (`"0"`, `"1"`, …) beside the tag field; untagged stores the payload bare and tries each variant in declaration order on load; `other` is a unit catch-all; `expecting` overrides the no-match error.
- Generics: `generics.rs` `Generics::analyze` propagates a type's generics + a default `T: SData` bound (or `#[strato(bound = "...")]`, which REPLACES it) onto the `SData`/`SIndexed` impls and the accessors (the latter gain a `PhantomData` over unused type params). Generated `store`/`load` name their params `__W`/`__R` to dodge a user param named `W`/`R`.
- `flatten` stores/loads the field AT the parent's node (its fields merge in); it is a compile error alongside any other field attribute.

**Known gaps:**
- `#[strato(packed)]` is NOT implemented; `#[strato(with = "Bytes")]` covers it (store a `Vec<u8>` as one `Bytes` leaf instead of shredding each byte).
- Tuple structs, unit structs, and unions still emit `compile_error!` on the normal path (the `from`/`into`/`try_from` path accepts them). Not planned.

### PAUSED — Milestone 4 (docs and polish)

- README (currently a one-liner)
- Crate-level rustdoc example showcasing indexes (currently only put/get in `lib.rs`)
- Any remaining cross-feature integration tests

Runnable examples are already done (`basic.rs`, `indexed.rs`).

---

## Deferred features

These are explicitly planned but not assigned to any current milestone. Do not implement them until explicitly requested.

**`rust_decimal` support** — `Decimal` would be added as a `Scalar` variant and a `SValue` impl, behind an optional Cargo feature `decimal`. Deferred until after milestone 4.

**Schema migration** — Today `$metadata` stores a `format_version` byte but no migration logic exists. A future migration layer would detect version mismatches on `StratoDb::open` and run a registered upgrade path. Not designed yet.

**Richer enum accessors** — Currently derived enums only expose `variant() -> String` (the active tag name); reading the payload requires `txn.load::<E>()`. A future enhancement could generate typed per-variant accessors (e.g., `as_foo() -> Option<StratoFoo<'t>>`). Not planned for any milestone; flagged as a possible unscheduled improvement.

**Relative path type (Abs/Rel split)** — A first-class `RelPath` type with deferred `..` resolution was discussed but deemed superfluous: the anchor-agnostic `SPath` combined with `join` / `resolve` / `rooted()` views already covers all real use cases. Closed as WONTFIX unless a concrete need arises.

---

## Known limitations

**Index maintenance skips list reorders.** `list_move` / `list_swap` (used by `SeqMut::swap`, `swap_remove`, `pop_first`, `pop_last`, `drain`, etc.) intentionally skip `reindex_around`. This is correct for wildcard patterns (`"users/*"`) because a reorder changes no key/column values. It is **incorrect** for a pattern containing a literal list index (e.g., `"items[0]"`): reordering elements silently produces a stale index. Wildcard patterns are the intended use; literal-index patterns in index scopes are not supported.

---

## Key invariants to preserve

- **redb stays opaque.** Never let a `redb::` type appear in the public API.
- **PathCache coherence.** `WriteTxn` MUST NOT use the cache. `ReadCursor` is the only cache-backed `Reader`. The four `Box<dyn Reader>` / `Arc<dyn Reader>` / `Box<dyn Writer>` / `Arc<dyn Writer>` forwarding impls MUST forward `child_cached` and `object_keys` explicitly (trait objects bypass the override otherwise).
- **Index maintenance brackets every mutation.** The delete-then-insert pattern in `reindex_around` means a whole-entity `store` is safe even for unique indexes (the entity's own prior entry is deleted before the new entry is inserted).
- **`OnceLock` not `OnceCell` on `WriteTxn.indexes`.** `OnceCell` is `!Sync`; `WriteTxn` must be `Sync` because `fetch_mut` returns an `Arc<WriteCursor>`.
- **Stored name vs getter name.** A field's getter method = Rust ident; its path segment = the effective stored name (rename > rename_all > ident). Never conflate the two.
- **Edition 2024 let-chains.** The codebase uses `if let … && …` chains freely; do not downgrade to nested `match`/`if let`.
- **`cargo +nightly fmt` only.** The `.rustfmt.toml` uses `struct_field_align_threshold`, `enum_discrim_align_threshold`, `imports_granularity`, and other nightly-only keys that stable fmt silently ignores.
