//! `#[derive(SData)]` for enums.
//!
//! Enums are stored **externally tagged**: the value's node is an object holding
//! exactly one field, named after the active variant, whose child is the
//! variant's payload. Payloads follow the variant shape:
//!
//! - unit `V`            -> `at/V` is a `Null` leaf;
//! - newtype `V(T)`      -> `at/V` is the stored `T` (directly);
//! - tuple `V(A, B)`     -> `at/V` is a list, `at/V[0]`, `at/V[1]`, ...;
//! - struct `V { a, b }` -> `at/V` is an object, `at/V/a`, `at/V/b`.
//!
//! The read/write accessors are intentionally minimal: they report the active
//! variant via `variant()`; recompose the whole value with `txn.load::<E>(path)`.

mod accessors;
mod expand_macro;
mod load_arm;
mod store_arm;
mod variant_parts;

pub(crate) use expand_macro::expand_enum;
