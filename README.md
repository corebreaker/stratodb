# StratoDB

A typed, transactional, indexed document store for Rust, layered over an embedded key-value engine.

StratoDB stores structured documents — objects, lists and scalar leaves — as a tree of individually-keyed nodes. Each value is addressable both by a stable opaque primary key and by a slash-separated path, so an entity keeps its identity through renames and moves. On top of that sit a derive macro for typed access, ordered secondary indexes, a dynamic document type, and JSON/YAML export — all with no storage-engine types leaking into the public API.

> **Status:** pre-1.0 (`0.1.x`), not yet released to crates.io. The architecture is locked and every capability below is implemented, tested, and documented. See [Project status](#project-status).

---

## Highlights

- **Full shredding.** Every scalar is its own node with its own key; nested objects and list elements are addressable in their own right (`users/alice/age`, `items[3]/name`).
- **Stable identity.** Nodes carry an opaque `Skey` (a UUIDv7) that survives renames and moves; paths are ephemeral addresses resolved by walking the tree.
- **Typed access.** Implement the `SData` trait by hand, or `#[derive(SData)]` with Serde-style `#[strato(...)]` attributes (rename, skip, default, custom (de)serialization, enum representations, generics, flatten).
- **Secondary indexes.** Named, composite, per-column ASC/DESC, optionally unique, scoped to a path pattern (`users/*`). The encoding is order-preserving, so prefix and range queries are correct.
- **Transactional.** Concurrent readers, a single serialized writer, snapshot-consistent reads, durable-on-commit writes.
- **Dynamic documents.** A `Value` tree mirrors stored data faithfully, with path-addressed get/set and load/store on a transaction.
- **Zero-dependency export.** Render any subtree to JSON or YAML through the `JsonExporter` / `YamlExporter` traits.
- **Opaque engine.** The underlying storage engine never appears in the public API.

---

## Installation

StratoDB is not yet published to crates.io. Depend on it from Git:

```toml
[dependencies]
stratodb = { git = "https://github.com/corebreaker/stratodb" }
```

Most users will want the derive macro:

```toml
[dependencies]
stratodb = { git = "https://github.com/corebreaker/stratodb", features = ["derive"] }
```

It builds on a recent stable Rust toolchain (edition 2024). See [Cargo features](#cargo-features) for the full matrix.

---

## The data model

A `StratoDb` is one database file holding any number of named **tables**. Inside a table, data is a tree of three node kinds:

| Node | Holds | Example source |
|------|-------|----------------|
| **Object** | named children | a struct, a `BTreeMap<String, _>` |
| **List** | ordered children | a `Vec<_>` |
| **Leaf** | one scalar | a number, string, bool, date, UUID, bytes, … |

Every node has an `Skey` primary key. The fixed root is `Skey::ROOT`. A **path** (`SPath`) is a slash-separated address — field names and bracketed list indices, e.g. `config/server/port` or `orders[0]/lines[2]/sku`. Paths are never persisted; they resolve by walking the tree at query time (with a per-table LRU cache on the read side), which is why an entity's identity follows its key, not its location.

Storing a value **shreds** it: a struct becomes an object node whose every field is its own child node, recursively down to the scalar leaves. The whole tree is reachable both as a typed value and field-by-field by path.

---

## Quick start

`create_in_memory()` keeps everything in RAM; for a persistent file use `StratoDb::create(path)` (or `StratoDb::open(path)` to reopen one).

```rust
use stratodb::{NodeKind, StratoDb};

fn main() -> stratodb::SdbResult<()> {
    let db = StratoDb::create_in_memory()?;
    let config = db.open_table("config")?;

    // Writes are transactional: stage, then commit.
    let w = config.write()?;
    w.put("server/host", &String::from("localhost"))?;
    w.put("server/port", &8080u32)?;
    w.put("server/tls", &true)?;
    w.commit()?;

    // Reads see committed data, decoded into the requested type.
    let r = config.read()?;
    assert_eq!(r.get::<String>("server/host")?, Some(String::from("localhost")));
    assert_eq!(r.get::<u32>("server/port")?, Some(8080));

    // Paths address a tree of nodes; `server` itself is an object node.
    assert_eq!(r.kind("server")?, Some(NodeKind::Object));
    Ok(())
}
```

---

## Typed data with `#[derive(SData)]`

With the `derive` feature, a Rust type stores and loads as a whole. The derive generates the `SData` implementation plus lazy accessors (`StratoXxx` / `StratoXxxMut`) for reading and mutating individual fields without loading the entire value.

```rust
use stratodb::{SData, StratoDb};

#[derive(SData, Debug, PartialEq)]
struct Profile {
    name:     String,
    age:      u32,
    tags:     Vec<String>,
    nickname: Option<String>,
}

fn main() -> stratodb::SdbResult<()> {
    let db = StratoDb::create_in_memory()?;
    let people = db.open_table("people")?;

    let alice = Profile {
        name:     String::from("Alice"),
        age:      30,
        tags:     vec![String::from("a"), String::from("b")],
        nickname: Some(String::from("al")),
    };

    let w = people.write()?;
    w.store("alice", &alice)?;       // decomposes the whole struct into nodes
    w.commit()?;

    let r = people.read()?;
    assert_eq!(r.load::<Profile>("alice")?, alice);

    // The shredded leaves are also reachable by raw path:
    assert_eq!(r.get::<u32>("alice/age")?, Some(30));
    assert_eq!(r.get::<String>("alice/tags[0]")?, Some(String::from("a")));
    Ok(())
}
```

Containers map as you'd expect: `Vec<T>` → a list of addressable nodes, `BTreeMap<String, T>` → an object, `Option<None>` → a present null leaf, and the `Bytes` newtype → a single leaf (vs. `Vec<u8>`, which shreds byte-by-byte). Enums are externally tagged by default.

### `#[strato(...)]` attributes

The derive supports a Serde-style attribute set under the `strato` namespace:

| Category | Attributes |
|----------|-----------|
| Renaming | `rename`, `rename_all` (8 casings), `alias` |
| Skip / default | `skip`, `skip_store`, `skip_load`, `skip_store_if`, `default` |
| Custom storage | `store_with`, `load_with`, `with` |
| Conversion | `from`, `into`, `try_from` |
| Generics | `bound` |
| Enum representation | `tag` (internal), `tag` + `content` (adjacent), `untagged`, `other` |
| Enum renaming | `rename_all` on the enum, `rename` / `alias` on variants |
| Misc | `expecting`, `flatten` |

```rust
use stratodb::SData;
#[derive(SData)]
#[strato(rename_all = "camelCase")]
struct Event {
    #[strato(rename = "ts")]
    timestamp: u64,
    #[strato(alias = "comment", default)]
    note:      String,
}
```

---

## Secondary indexes

An index is **named**, **composite** (ordered columns), per-column **ASC/DESC**, optionally **unique**, and **scoped** to a path pattern (`*` matches one segment). Indexes are maintained automatically on every write and back-filled when first created, so they are correct whether data was written before or after the index existed.

Define one explicitly:

```rust
use std::collections::BTreeMap;
use stratodb::{data::Scalar, index::{IndexColumn, IndexDef}, path::SPath, StratoDb};

fn main() -> stratodb::SdbResult<()> {
    let db = StratoDb::create_in_memory()?;
    let users = db.open_table("users")?;

    users.create_index(&IndexDef::new(
        String::from("by_age"),
        String::from("users/*"),
        vec![IndexColumn::asc(SPath::parse("age")?)],
        false, // not unique
    ))?;

    let w = users.write()?;
    w.put("users/alice/age", &30u32)?;
    w.put("users/bob/age", &30u32)?;
    w.put("users/carol/age", &40u32)?;
    w.commit()?;

    let r = users.read()?;
    // Each hit is recomposed from its own subtree.
    let at_30: Vec<BTreeMap<String, u32>> = r.find("by_age", &[Scalar::U32(30)])?;
    assert_eq!(at_30.len(), 2);
    Ok(())
}
```

Or declare indexes right on a derived type and register them in one call:

```rust
use stratodb::{data::Scalar, SData, StratoDb};
#[derive(SData)]
#[strato(index(name = "by_team", columns(team)))]
#[strato(index(name = "by_email", columns(email), unique))]
struct Member {
    name:  String,
    team:  String,
    email: String,
}

# fn main() -> stratodb::SdbResult<()> {
# let db = StratoDb::create_in_memory()?;
let members = db.open_table("members")?;
members.create_indexes::<Member>("members/*")?;   // both indexes, scoped + back-filled
# Ok(())
# }
```

`find` is the common exact/prefix lookup. For reverse order, partial (prefix) matches, or a subtree scope, build a query:

```rust
use stratodb::{data::Scalar, txn::ReadTxn};
fn demo(r: &ReadTxn) -> stratodb::SdbResult<()> {
    let newest: Vec<u64> = r.query("by_created")
        .prefixed(&[Scalar::Str(String::from("eng"))])  // leading column(s)
        .reversed()                                      // descending
        .run()?;
    Ok(())
}
```

A unique index rejects a second entity producing the same column tuple with `SdbError::UniqueViolation`; the offending write rolls back.

Indexes can be ensured, inspected, and dropped:

```rust
use stratodb::{index::IndexDef, Table};
fn demo(members: &Table, def: &IndexDef) -> stratodb::SdbResult<()> {
    members.ensure_index(def)?;             // create + back-fill only if absent; no-op (no error) if a same-named index exists
    members.ensure_indexes::<Member>("members/*")?;   // same, for every index the type declares

    members.has_index("by_team")?;          // presence check — no IndexDef is deserialized
    members.index_def("by_team")?;          // the full definition, if it exists

    members.delete_index("by_team")?;       // drop one: registration + every entry, atomically; returns whether it existed
    members.delete_indexes::<Member>()?;    // drop every index a type declares (mirror of create_indexes); returns the count removed
    Ok(())
}
```

`ensure_index` / `ensure_indexes` are the idempotent-by-name counterparts of `create_index` / `create_indexes`: an absent index is created and back-filled, but a name already in use is left exactly as it is — unlike `create_index`, which errors with `SchemaMismatch` on a divergent redefinition. `has_index` is optimized for presence alone — it scans the registry without materializing any `IndexDef` (no column path is parsed). `delete_index` removes the registry record and purges every entry the index holds in one transaction; it is idempotent (`false` when no such index exists). Both `delete_*` leave the indexed data untouched.

---

## Dynamic documents — `Value`

`Value` is an in-memory mirror of the node tree (`Leaf(Scalar)` / `List` / `Node`), useful when the shape isn't known at compile time. It is **faithful** — each leaf keeps its exact scalar — and carries path-addressed access that never destroys data:

```rust
use stratodb::{data::Scalar, StratoDb, Value};

fn main() -> stratodb::SdbResult<()> {
    let db = StratoDb::create_in_memory()?;
    let table = db.open_table("data")?;

    // Load a stored subtree as a Value, edit it, store it back.
    let w = table.write()?;
    w.put("user/age", &30u32)?;
    w.commit()?;

    let r = table.read()?;
    let mut user = r.load_value("user")?.unwrap();
    user.set_value("city", Value::Leaf(Scalar::Str(String::from("Bern"))));

    let w = table.write()?;
    w.store_value("user", &user)?;   // replace semantics + full index maintenance
    w.commit()?;
    Ok(())
}
```

`set_value` is atomic and never-destructive: it creates missing containers along the way but refuses (returning `false`, mutating nothing) to traverse through or overwrite a leaf mid-path, or to grow a list past its end. `get_value` returns a clone of the subtree at a path.

---

## Export — JSON & YAML

The `export` module renders any stored or in-memory subtree to text, with no external dependency. Both `ReadTxn` (stored data) and `Value` (in-memory) implement the same two traits:

```rust
use stratodb::{export::{JsonExporter, YamlExporter}, StratoDb};

fn main() -> stratodb::SdbResult<()> {
    let db = StratoDb::create_in_memory()?;
    let table = db.open_table("data")?;

    let w = table.write()?;
    w.put("user/name", &String::from("Alice"))?;
    w.put("user/age", &30u32)?;
    w.commit()?;

    let r = table.read()?;
    assert_eq!(r.export_to_json("user", None)?, r#"{"age":30,"name":"Alice"}"#);   // compact
    let pretty = r.export_to_json("user", Some(2))?;                               // 2-space pretty
    let yaml = r.export_to_yaml("user")?;                                          // block style
    Ok(())
}
```

Object fields come out in sorted order. Scalars without a native JSON/YAML form take a textual one: dates/times as ISO 8601 / RFC 3339, a UUID hyphenated, raw bytes as Base64, a duration as decimal seconds, a rational as `num/den`, and the non-finite floats as `null`. This rendering step is the only lossy part of an export.

---

## Big numbers

Behind the `bignum` feature family, three arbitrary-precision types become first-class: `num_bigint::BigInt`, `num_bigfloat::BigFloat` (a fixed 40-digit decimal float), and `num_rational::BigRational`. Each has two orthogonal axes — `*-as-scalar` (a native `Scalar` variant + `SValue`) and `*-as-data` (an `SData` impl) — with `bignum` turning on everything. The order-preserving index codecs sort all three by value, so range and unique indexes stay correct.

```toml
stratodb = { git = "https://github.com/corebreaker/stratodb", features = ["bignum"] }
```

---

## Cargo features

| Feature | Default | Pulls in | Effect |
|---------|:-------:|----------|--------|
| `derive` | — | `stratodb-derive` | `#[derive(SData)]` and `#[strato(...)]` attributes |
| `bignum` | — | both umbrellas below | every big-number type, as scalar **and** data |
| `bignum-as-scalar` | — | the three `*-as-scalar` | big-number `Scalar` variants + `SValue` |
| `bignum-as-data` | — | the three `*-as-data` | big-number `SData` impls (stored as one `Bytes` leaf when not also a scalar) |
| `bigint-as-scalar` / `bigfloat-as-scalar` / `rational-as-scalar` | — | the matching `num-*` crate | one scalar type as a `Scalar` |
| `bigint-as-data` / `bigfloat-as-data` / `rational-as-data` | — | the matching `num-*` crate | one scalar type as `SData` |

Nothing is on by default.

---

## Transactions & concurrency

- `table.read()` opens a snapshot-consistent read transaction; many may run concurrently with each other and with a writer.
- `table.write()` opens the single active write transaction; changes are visible to that transaction immediately and become durable — and visible to new readers — only on `commit()`. Dropping (or `abort()`) discards them.
- Index maintenance is bracketed around every mutation (delete affected entries → apply → re-insert), so a whole-entity replace is safe even under unique indexes.

---

## Examples

Runnable examples live in [`crates/stratodb/examples`](crates/stratodb/examples):

```sh
cargo run -p stratodb --example basic
cargo run -p stratodb --example indexed --features derive
```

---

## Project status

| Area | State |
|------|-------|
| Core store (tables, paths, nodes, transactions, path cache) | ✅ |
| `SData` trait, accessors, container types | ✅ |
| `#[derive(SData)]` + the full `#[strato(...)]` attribute set | ✅ |
| Secondary indexes (composite, unique, ordered, back-filled) | ✅ |
| Big-number scalars (`bignum`) | ✅ |
| Dynamic `Value` + JSON/YAML export | ✅ |
| Documentation (README, crate rustdoc, runnable examples) | ✅ |

Everything above is complete; the next step toward 1.0 is a crates.io release. Planned but not yet scheduled: `rust_decimal` support, schema migration, richer typed enum accessors.

---

## License

Licensed under the [MIT License](LICENSE).
