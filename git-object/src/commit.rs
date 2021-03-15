use crate::borrowed;
use bstr::{BStr, ByteSlice};

/// An iterator over extra headers in [owned][crate::owned::Commit] and [borrowed][borrowed::Commit] commits.
pub struct ExtraHeaders<I> {
    inner: I,
}

/// Instantiation and convenience.
impl<'a, I> ExtraHeaders<I>
where
    I: Iterator<Item = (&'a BStr, &'a BStr)>,
{
    /// Create a new instance from an iterator over tuples of (name, value) pairs.
    pub fn new(iter: I) -> Self {
        Self { inner: iter }
    }
    /// Find the _value_ of the _first_ header with the given `name`.
    pub fn find(mut self, name: &str) -> Option<&'a BStr> {
        self.inner
            .find_map(move |(k, v)| if k == name.as_bytes().as_bstr() { Some(v) } else { None })
    }
    /// Return an iterator over all _values_ of headers with the given `name`.
    pub fn find_all(self, name: &'a str) -> impl Iterator<Item = &'a BStr> {
        self.inner
            .filter_map(move |(k, v)| if k == name.as_bytes().as_bstr() { Some(v) } else { None })
    }
    /// Return an iterator over all git mergetags.
    ///
    /// A merge tag is a tag object embedded within the respective header field of a commit, making
    /// it a child object of sorts.
    pub fn mergetags(self) -> impl Iterator<Item = Result<borrowed::Tag<'a>, borrowed::Error>> {
        self.find_all("mergetag").map(|b| borrowed::Tag::from_bytes(b))
    }

    /// Return the cryptographic signature provided by gpg/pgp verbatim.
    pub fn pgp_signature(self) -> Option<&'a BStr> {
        self.find("gpgsig")
    }
}
