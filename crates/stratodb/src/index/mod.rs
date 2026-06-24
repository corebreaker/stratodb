//! Secondary indexes.
//!
//! Built incrementally across milestone 3: order-preserving key encoding first,
//! then index definitions, the `$metadata` registry, write-time maintenance and
//! queries.

// The codec is fully exercised by its own tests; its first non-test caller
// arrives with index maintenance (a later milestone-3 sub-step). Remove this
// allow once it is wired in.
#[allow(dead_code)]
pub(crate) mod ordered;
