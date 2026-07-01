[![Crates.io](https://img.shields.io/crates/v/stratodb-derive?style=for-the-badge)](https://crates.io/crates/stratodb-derive)
[![Docs.rs](https://img.shields.io/docsrs/stratodb?style=for-the-badge)](https://docs.rs/stratodb/)

# stratodb-derive

Procedural macros for [StratoDB](https://crates.io/crates/stratodb) — the `#[derive(SData)]` macro and its Serde-style `#[strato(...)]` attributes.

> **Do not depend on this crate directly.** It is an implementation detail of StratoDB and carries no stable API of its own. The macro is re-exported by the main crate behind its `derive` feature, so one import brings both the `SData` trait and the derive into scope.

## Usage

Enable the `derive` feature on `stratodb`:

```toml
[dependencies]
stratodb = { version = "1.0", features = ["derive"] }
```

```rust
use stratodb::SData;

#[derive(SData)]
struct User {
    name: String,
    age:  u32,
}
```

The full guide — every `#[strato(...)]` attribute (rename, skip, default, custom (de)serialization, enum representations, generics, flatten), typed indexes, and the rest of StratoDB — lives with the main crate:

- **Documentation:** <https://docs.rs/stratodb/>
- **Repository:** <https://github.com/corebreaker/stratodb>

## License

MIT — same as StratoDB.
