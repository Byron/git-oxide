//! Read and write the git packet line wire format without copying it.
//!
//! For reading the packet line format use the [`StreamingPeekableIter`], and for writing the `Writer`.
#![deny(unsafe_code, rust_2018_idioms, missing_docs)]

const U16_HEX_BYTES: usize = 4;
const MAX_DATA_LEN: usize = 65516;
const MAX_LINE_LEN: usize = MAX_DATA_LEN + U16_HEX_BYTES;
const FLUSH_LINE: &[u8] = b"0000";
const DELIMITER_LINE: &[u8] = b"0001";
const RESPONSE_END_LINE: &[u8] = b"0002";
const ERR_PREFIX: &[u8] = b"ERR ";

/// One of three side-band types allowing to multiplex information over a single connection.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub enum Channel {
    /// The usable data itself in any format.
    Data = 1,
    /// Progress information in a user-readable format.
    Progress = 2,
    /// Error information in a user readable format. Receiving it usually terminates the connection.
    Error = 3,
}

///
pub mod immutable;
pub use immutable::PacketLine;

///
pub mod read;
#[doc(inline)]
pub use read::StreamingPeekableIter;

///
#[cfg(any(feature = "async-io", feature = "blocking-io"))]
pub mod write;
#[cfg(any(feature = "async-io", feature = "blocking-io"))]
#[doc(inline)]
pub use write::Writer;

/// Utilities to help decoding packet lines
pub mod decode;
#[doc(inline)]
pub use decode::all_at_once as decode;
/// Utilities to encode different kinds of packet lines
pub mod encode;

#[cfg(all(feature = "async-io", feature = "blocking-io"))]
compile_error!("Cannot set both 'blocking-io' and 'async-io' features as they are mutually exclusive");
