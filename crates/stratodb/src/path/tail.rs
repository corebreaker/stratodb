use super::SPath;

/// A value that can be appended to an [`SPath`] with `/` or `/=`.
///
/// A path tail (`SPath`/`&SPath`) joins segment-wise (like [`SPath::join`]); a
/// string tail (`&str`/`String`) appends a single field name (like
/// [`SPath::child_name`], with the same `.`/`..` handling). For an index or a
/// multi-segment fragment, parse the string first: `path / SPath::parse("t[0]")?`.
pub trait PathTail {
    /// Appends this value's segment(s) onto `path`.
    fn append_to(self, path: &mut SPath);
}

impl PathTail for SPath {
    fn append_to(self, path: &mut SPath) {
        path.inplace_join(&self);
    }
}

impl PathTail for &SPath {
    fn append_to(self, path: &mut SPath) {
        path.inplace_join(self);
    }
}

impl<S: AsRef<str>> PathTail for S {
    fn append_to(self, path: &mut SPath) {
        path.push_name(self.as_ref());
    }
}
