/// One component of an [`SPath`](super::SPath).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Segment {
    /// An object field name, e.g. `h` in `a/h`.
    Name(String),
    /// A zero-based list index, e.g. `5` in `a/t[5]`.
    Index(u64),
}
