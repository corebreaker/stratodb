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
│   │   ├── examples/           runnable examples (basic.rs, indexed.rs)
│   │   └── Cargo.toml
│   └── stratodb-derive/        proc-macro crate (#[derive(SData)])
│       └── src/
```

The workspace is just the two crates (`members = ["crates/*"]`, no `exclude`). Examples live under `crates/stratodb/examples/` and are declared as `[[example]]` targets in `crates/stratodb/Cargo.toml` (`indexed` carries `required-features = ["derive"]`).

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

The big-number features need one extra spot-check: `--all-features` turns every `*-as-scalar` on, which compiles out the `*-as-data`-only path in `data/bignum.rs`. Exercise it explicitly with `cargo test -p stratodb --features bignum-as-data` and `cargo clippy --all-targets --features bignum-as-data -- -D warnings`.

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

**Index model.** Secondary indexes are named, composite (ordered columns), per-column ASC/DESC, with optional uniqueness. Non-unique: entity in the key (`INDEX_DUP` tag). Unique: entity in the value (`INDEX_UNIQUE` tag); collision = `UniqueViolation`. Scope = path pattern (e.g., `"users/*"`); `*` is a one-segment wildcard. Order-preserving encoding: DESC = bitwise complement; strings use a two-byte `0x00 0x01` terminator (escape `0x00` → `0x00 0xFF`) so encodings are prefix-free. Lifecycle: `create_index`/`create_indexes::<T>` register + back-fill (error on a divergent same-name redefinition); `ensure_index`/`ensure_indexes::<T>` are the idempotent-by-name variant (create + back-fill if absent, no-op — no error — if a same-name index already exists, whatever its definition); `index_def`/`has_index` introspect (`has_index` is a name-only registry scan that never materializes an `IndexDef`); `delete_index`/`delete_indexes::<T>` drop, removing the registry record and purging every physical entry in one transaction. Dropping leaves `next_id` untouched — index ids are never reused.

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
├── table.rs                Table handle → read()/write()/create_index(es)/ensure_index(es)/index_def/has_index/delete_index(es)
├── tree.rs                 tree walk, node resolution, list helpers
├── value.rs                Value enum — dynamic Leaf/List/Node tree + get_value/set_value/subtree
├── codec/                  byte encoding (putters, reader)
├── engine/                 redb table defs, META_TABLE, TableKey/TableValue encoding
├── access/
│   ├── reader.rs           ReadCursor + Reader trait (get_node, child_cached, object_keys…)
│   ├── writer.rs           WriteCursor + Writer trait (put_node, ensure_container…)
│   └── rooted.rs           Rooted<R> adapter (re-roots SData::load at an entity key)
├── data/
│   ├── definition.rs       SData trait (store/load)
│   ├── value.rs            SValue trait + macro_rules for scalar impls
│   ├── scalar.rs           Scalar enum (21 base variants + optional BigInt/BigFloat/Rational behind bignum-as-scalar)
│   ├── sref.rs / smut.rs   SRef / SMut bound traits
│   ├── leaf_ref.rs / leaf_mut.rs  Leaf<'t,T> / LeafMut<'t,T>
│   ├── seq.rs              Vec<T> → Seq / SeqMut
│   ├── map.rs              BTreeMap<String,T> → Map / MapMut
│   ├── opt.rs              Option<T> → OptRef / OptMut
│   ├── bytes.rs            Bytes newtype
│   ├── bignum.rs           SData for BigInt/BigFloat/BigRational as a single Bytes leaf (the -as-data-only path)
│   └── identifiable.rs     SIdentifiable (key + path from an accessor)
├── path/
│   ├── spath.rs            SPath (immutable slash-separated path; parse normalises ./.. )
│   ├── segment.rs          Segment (field name or list index)
│   ├── functions.rs
│   ├── into_path.rs        IntoPath trait — path args accept &str/String/SPath (parsed or used as-is)
│   └── tail.rs             PathTail trait + / and /= operators on SPath
├── index/
│   ├── definitions/        IndexDef, IndexColumn, Direction
│   ├── registry/           $metadata registry (create/lookup/list/has/delete by table; has = name-only scan, no IndexDef materialized)
│   ├── id.rs               IndexId
│   ├── indexed.rs          SIndexed trait
│   ├── ordered.rs          order-preserving Scalar codec (incl. bignum: length-prefixed int, decimal-float, continued-fraction rational)
│   ├── pattern.rs          Pattern (*-wildcard) + affected_entities(scope)
│   └── maintenance.rs      delete + insert (bracket every mutation); delete_all (purge every entry of one index when dropped)
├── export/                 JSON/YAML rendering of a Value (the JsonExporter/YamlExporter traits)
│   ├── exporter.rs         JsonExporter/YamlExporter traits + impls for ReadTxn and Value
│   ├── json.rs             to_json(&Value, indent) — compact / pretty
│   ├── yaml.rs             to_yaml(&Value) — block style
│   ├── scalar.rs           write_scalar — the single lossy Scalar→text step
│   ├── string.rs           shared double-quoted string escaping
│   └── base64.rs           minimal RFC 4648 Base64 encoder (Bytes leaves)
└── txn/
    ├── read.rs             ReadTxn (get/kind/fetch/load/find/query/rooted)
    ├── write.rs            WriteTxn (put/store/remove/commit/rooted + reindex_around)
    ├── value.rs            ReadTxn::load_value / WriteTxn::store_value (Value ↔ tree; read_value shared with export)
    ├── query.rs            IndexQuery builder (.prefixed/.reversed/.under/.run)
    └── rooted/
        ├── read.rs         RootedRead<'a> (borrows ReadTxn, relative paths)
        └── write.rs        RootedWrite<'a> (borrows WriteTxn, relative paths)
```

**Public re-exports** (top-level `use stratodb::…`):
`StratoDb`, `Table`, `SData` (trait + derive macro with feature), `Skey`, `NodeKind`, `Value`, `SdbError`, `SdbResult`.
Plus `pub mod`: `data`, `error`, `index`, `path`, `txn`, `access`, `constants`, `export` (the `JsonExporter` / `YamlExporter` traits).

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
| `tests/indexes.rs` | — | index registry, maintenance, query builder, unique enforcement, `ensure_index` (create-if-absent, no-op on present/divergent), `has_index`/`delete_index` (registry purge + physical entries, idempotent, other indexes & data intact, recreate, reopen) |
| `tests/export.rs` | — | JSON/YAML export of stored subtrees (compact/pretty/block, scalar rendering, missing path, scalar & list roots) |
| `tests/value.rs` | — | dynamic `Value`: `store_value`/`load_value` round-trips, `get_value`/`set_value`, `Value`'s own `JsonExporter`/`YamlExporter` |
| `tests/derive.rs` | `derive` | #[derive(SData)]: structs/enums + every `#[strato(...)]` attr (rename/skip/default/with, from/into/try_from, enum reps, generics+bound, flatten) |
| `tests/index_typed.rs` | `derive` | end-to-end derived indexes (back-fill, composite prefix, unique, reopen) + `ensure_indexes::<T>()` (creates missing, skips present, idempotent) + `delete_indexes::<T>()` (drops every declared index, returns the count, idempotent) |
| `tests/cross_feature.rs` | `derive` (+ `bignum`) | feature seams together: a derived+renamed entity with an enum field, indexed (unique + non-unique), exported to JSON/YAML, round-tripped through `Value`; a `#[cfg(feature = "bignum")]` module covers a BigInt index ordering by value and bignum scalars exporting |

Big-number coverage lives in `src` unit tests, not a `tests/` file: `data/scalar.rs` (storage round-trips), `index/ordered.rs` (value ordering), and `data/bignum.rs` (as-data round-trips via an in-memory DB, gated on a `*-as-data`-only combo).

The export writers also carry `src` unit tests: `export/scalar.rs` (each `Scalar`'s text form), `export/json.rs` / `export/yaml.rs` (layout + escaping on hand-built `Value`s), and `export/base64.rs` (RFC 4648 vectors).

---

## Milestone roadmap

### COMPLETE

| Milestone | Description |
|-----------|-------------|
| 1 | Foundation: StratoDb, Table, ReadTxn, WriteTxn, SPath, Skey, Node, tree walk, path cache |
| 2 | SData trait + accessors: SValue/Scalar, Leaf/LeafMut, Vec/Option/BTreeMap/Bytes containers, #[derive(SData)] for structs and enums, StratoXxxDesc |
| 3 | Secondary indexes: order-preserving codec, IndexDef + registry, maintenance, pattern matching, query builder, unique enforcement, #[strato(index(...))] derive attr, back-fill |
| derive-attrs | Serde-style `#[strato(...)]` attributes — 7 phases (detailed below) |
| bignum | Optional BigInt / BigFloat / BigRational as `Scalar`/`SValue`/`SData` + order-preserving index codecs (detailed below) |
| export + Value | Hand-rolled (zero-dep) JSON/YAML export via the `JsonExporter`/`YamlExporter` traits + a dynamic `Value` document type with load/store and path get/set (detailed below) |

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

### COMPLETE — Big-number scalars (`bignum`)

Optional support for `num_bigint::BigInt`, `num_bigfloat::BigFloat` (a fixed 40-digit **decimal** float), and `num_rational::BigRational`, behind a feature matrix (`default = []`). Each type has two orthogonal axes — **`-as-scalar`** (native `Scalar` variant + `SValue`) and **`-as-data`** (`SData` impl) — with umbrellas rolling them up:

| Feature | Pulls in | Effect |
|---------|----------|--------|
| `bigint-as-scalar` / `bigfloat-as-scalar` / `rational-as-scalar` | the matching `num-*` crate | a `Scalar` variant + `SValue` impl |
| `bigint-as-data` / `bigfloat-as-data` / `rational-as-data` | the matching `num-*` crate | an `SData` impl |
| `bignum-as-scalar` / `bignum-as-data` | the three above, respectively | — |
| `bignum` | both umbrellas | everything |

`rational-as-scalar` and `rational-as-data` also pull in `num-bigint` — a rational is (de)serialised through its `BigInt` numerator/denominator.

**Two storage representations, chosen by feature combo:**
- **`-as-scalar` + `-as-data`** (e.g. under `bignum`): `SData` is the `scalar_sdata!` macro in `value.rs` — the value stores as one native `Scalar` leaf.
- **`-as-data` only** (no matching `-as-scalar`): the type is not a `Scalar`, so `data/bignum.rs` provides `SData` by serialising to a single `Bytes` leaf (BigInt = signed-BE; BigRational = length-prefixed numer+denom; BigFloat = tag + sign/exponent/mantissa). `Ref`/`Mut` delegate to `Bytes`'s accessors, so `acc.get()` returns raw `Bytes` — recompose the typed value with `txn.load::<T>(path)`.

**Exact-storage codec** (`scalar.rs`, round-trippable): BigFloat keys the special values by tag (`NaN`/`±∞`/`0`) and stores a finite value as sign + `i8` exponent + decimal-digit mantissa. The check order is `NaN → +∞ → −∞ → 0 → finite` (a sign test alone cannot separate the two infinities).

**Order-preserving index codec** (`ordered.rs`): all three sort by value, so range and unique indexes are both correct.
- **BigInt**: a sign-class byte (`neg < zero < pos`) then a length-prefixed magnitude (negatives bit-inverted). This is NOT the fixed-width `signed()` helper, which inverts order across a byte-length boundary (it sorted `127` above `128`).
- **BigFloat**: a value-ordered class tag (`−∞ < neg < 0 < pos < +∞`, NaN parked at the top of the block) then, for finite values, the base-10 exponent of the leading digit followed by the significant digits; the negative body is inverted.
- **BigRational**: a continued-fraction (Stern-Brocot) encoding — `a0` via the signed-int codec, later terms length-prefixed with **odd-index terms bit-inverted** (a continued fraction decreases in its odd-position terms), and a parity-chosen stop marker so termination sorts on the correct side. The canonical CF (last term ≥ 2) is unique per value, so unique indexes hold too.

**Tested:** storage round-trips in `scalar.rs`; as-data round-trips in `bignum.rs` (via an in-memory DB, under a `*-as-data`-only combo); value ordering in `ordered.rs` (`bigints_order` / `bigfloats_order` / `rationals_order`, each asserting strict ascending **and** descending).

**Known gaps:**
- BigFloat is num-bigfloat's fixed 40-digit decimal float, not arbitrary-precision binary.
- A `-as-data`-only accessor's `get()` returns `Bytes`, not the typed value (mirrors the `from`/`into` derive philosophy, where accessors delegate to the target representation).

### COMPLETE — Textual export and the dynamic `Value`

Hand-rolled JSON/YAML export (no serde, no external dependency) plus a dynamic, in-memory document type, `Value`.

**`Value`** (`value.rs`, re-exported at the crate root) — the dynamic mirror of the node tree: `Leaf(Scalar)` / `List(Vec<Value>)` / `Node(BTreeMap<String, Value>)`. It is **faithful** (each leaf keeps its exact `Scalar`), unlike the export projection. Beyond the in-memory helpers (`get`/`at`/`insert`/`push`/`merge`/…), it carries path-addressed access:
- `get_value(path) -> Option<Value>` — a clone of the subtree at `path`; `None` if a segment leads nowhere, a list index is out of range, or the string does not parse. The root path returns the whole value.
- `set_value(&mut self, path, value) -> bool` — **atomic and never-destructive**: it creates missing containers (a `Name` segment makes an object, an `Index` a list) and replaces the value *at the destination* (a leaf there IS overwritten — that is the point of a set), but returns `false` and leaves `self` untouched if a segment would traverse an existing leaf *mid-path*, hit the wrong container kind, or grow a list past its end. Built by descending existing nodes with `get_mut` and attaching a freshly-built subtree (`build_fresh`) only on success, so a deep conflict mutates nothing. A fresh list only accepts index `0` (it starts empty).
- `subtree(&self, &SPath) -> Option<&Value>` (`pub(crate)`) — the borrowing walk shared by `get_value` and the exporter.

**Load/store on transactions** (`txn/value.rs`):
- `ReadTxn::load_value(path) -> Option<Value>` — walks the resolved subtree into a `Value`; `None` if absent. The walk (`read_value`, `pub(crate)`) is shared with the exporters.
- `WriteTxn::store_value(path, &Value)` — decomposes a `Value` back into nodes with replace semantics and full index maintenance (it goes through `WriteCursor`, exactly like the typed `store`).

**Export** (`export/` — a public module, `stratodb::export`):
- Two traits — `JsonExporter { export_to_json(path, indent) }` and `YamlExporter { export_to_yaml(path) }` — implemented by **`ReadTxn`** (renders the stored subtree at `path`; the root of an empty table → `null`, any other absent path → `PathNotFound`) and by **`Value`** (navigates the in-memory subtree at `path`; absent → `PathNotFound`, root → the whole value). `impl IntoPath` in argument position makes the traits **non-dyn-compatible** (intentional; no `dyn` use).
- The writers walk a `&Value`: `json.rs` (compact when `indent` is `None`, `n`-space pretty for `Some(n)`) and `yaml.rs` (block style, every string double-quoted). Object fields always come out in sorted (`BTreeMap`) order.
- The **only lossy step** is `scalar.rs::write_scalar`, the single place a `Scalar` becomes text: numbers and booleans verbatim, `null` for null and the non-finite floats (`NaN`, `±∞`), double-quoted otherwise — dates/times as ISO 8601 / RFC 3339, a UUID hyphenated, `Bytes` → Base64 (the minimal `base64.rs`), a duration → decimal seconds, a rational → `num/den`.

There is exactly **one** dynamic value type: an earlier `ExportValue` was folded into `Value` (the export now projects each leaf's `Scalar` at render time instead of pre-projecting into a second type).

### Milestone 4 (docs and polish) — mostly done

- **DONE** — README: a full guide (overview, data model, quickstart, derive + `#[strato(...)]` table, indexes, dynamic `Value`, JSON/YAML export, big numbers, the Cargo-feature matrix, transactions, examples, status).
- **DONE** — Crate-level rustdoc (`lib.rs`): runnable getting-started + secondary-index examples (the index one is feature-agnostic — it recomposes hits as `BTreeMap<String, u32>` so the doctest needs no `derive`) plus a "what else is here" tour. Doctests only ever run under `--all-features --doc` in the gate.
- **DONE** — Cross-feature integration tests (`tests/cross_feature.rs`).
- Runnable examples were already done (`basic.rs`, `indexed.rs`).

`cargo doc` is warning-clean across the workspace and every feature set (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` with `--features derive` / `--all-features` / none all pass) — a one-time sweep fixed every broken/private intra-doc link in both crates. This check is **not** part of the standard gate; re-run it after touching doc-comments. Links to private items (`PathCache`, `crate::tree`, the private `index_attr`/`query` modules) were turned into plain code spans or dropped, since rustdoc rejects a public item linking to a private one.

---

## Deferred features

These are explicitly planned but not assigned to any current milestone. Do not implement them until explicitly requested.

**`rust_decimal` support** — `Decimal` would be added as a `Scalar` variant and a `SValue` impl, behind an optional Cargo feature `decimal` (following the big-number feature pattern above). Deferred until after milestone 4.

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
- **bignum index codecs sort by value — do not "simplify" them.** `ordered.rs` reassigns the BigFloat class tags into value order (unlike the storage tags in `scalar.rs`, where order is irrelevant), encodes BigInt with a length-prefixed magnitude (NOT the fixed-width `signed()` helper), and encodes rationals as continued fractions. Reverting any of these to a fixed-width or bare-magnitude scheme silently corrupts index order across byte-length boundaries.
- **One dynamic value type.** `Value` (faithful: `Leaf(Scalar)`/`List`/`Node`) is the only dynamic document type. The export writers project each leaf at render time through the single, lossy `export/scalar.rs::write_scalar` site — do NOT reintroduce a parallel "export value" type. `ReadTxn::read_value` is the one tree→`Value` walk, shared by `load_value` and the exporters.
