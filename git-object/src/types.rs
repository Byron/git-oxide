use crate::owned::SPACE;
use quick_error::quick_error;
use std::{fmt, io};

/// Indicates if a number is positive or negative for use in [`Time`].
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
#[allow(missing_docs)]
pub enum Sign {
    Plus,
    Minus,
}

/// A timestamp with timezone.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Time {
    /// time in seconds from epoch.
    pub time: u32,
    /// time offset in seconds, may be negative to match the `sign` field.
    pub offset: i32,
    /// the sign of `offset`, used to encode `-0000` which would otherwise loose sign information.
    pub sign: Sign,
}

impl Time {
    /// Serialize this instance to `out` in a format suitable for use in header fields of serialized git commits or tags.
    pub fn write_to(&self, mut out: impl io::Write) -> io::Result<()> {
        itoa::write(&mut out, self.time)?;
        out.write_all(SPACE)?;
        out.write_all(&[match self.sign {
            Sign::Plus => b'+',
            Sign::Minus => b'-',
        }])?;

        const ZERO: &[u8; 1] = b"0";

        const SECONDS_PER_HOUR: i32 = 60 * 60;
        let offset = self.offset.abs();
        let hours = offset / SECONDS_PER_HOUR;
        assert!(hours < 25, "offset is more than a day: {}", hours);
        let minutes = (offset - (hours * SECONDS_PER_HOUR)) / 60;

        if hours < 10 {
            out.write_all(ZERO)?;
        }
        itoa::write(&mut out, hours)?;

        if minutes < 10 {
            out.write_all(ZERO)?;
        }
        itoa::write(&mut out, minutes).map(|_| ())
    }
}

/// The four types of objects that git differentiates.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
#[allow(missing_docs)]
pub enum Kind {
    Tree,
    Blob,
    Commit,
    Tag,
}
quick_error! {
    /// The Error used in [`Kind::from_bytes()`].
    #[derive(Debug)]
    #[allow(missing_docs)]
    pub enum Error {
        InvalidObjectKind(kind: crate::BString) {
            display("Unknown object kind: {:?}", std::str::from_utf8(kind))
        }
    }
}

impl Kind {
    /// Parse a `Kind` from its serialized loose git objects.
    pub fn from_bytes(s: &[u8]) -> Result<Self, Error> {
        Ok(match s {
            b"tree" => Self::Tree,
            b"blob" => Self::Blob,
            b"commit" => Self::Commit,
            b"tag" => Self::Tag,
            _ => return Err(Error::InvalidObjectKind(s.into())),
        })
    }

    /// Return the name of `self` for use in serialized loose git objects.
    #[must_use]
    pub fn to_bytes(&self) -> &[u8] {
        match self {
            Self::Tree => b"tree",
            Self::Commit => b"commit",
            Self::Blob => b"blob",
            Self::Tag => b"tag",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(std::str::from_utf8(self.to_bytes()).expect("Converting Kind name to utf8"))
    }
}

///
pub mod tree {
    /// The mode of items storable in a tree, similar to the file mode on a unix file system.
    ///
    /// Used in [owned::Entry][crate::owned::tree::Entry] and [borrowed::Entry][crate::borrowed::tree::Entry].
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Ord, PartialOrd, Hash)]
    #[repr(u16)]
    #[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
    #[allow(missing_docs)]
    pub enum Mode {
        Tree = 0o040_000_u16,
        Blob = 0o100_644,
        BlobExecutable = 0o100_755,
        Link = 0o120_000,
        Commit = 0o160_000,
    }
}
