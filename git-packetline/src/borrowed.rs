use crate::{encode, Channel, ERR_PREFIX};
use bstr::BStr;
use std::io;

/// A borrowed packet line as it refers to a slice of data by reference.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub enum Borrowed<'a> {
    /// A chunk of raw data.
    Data(&'a [u8]),
    /// A flush packet.
    Flush,
    /// A delimiter packet.
    Delimiter,
    /// The end of the response.
    ResponseEnd,
}

impl<'a> Borrowed<'a> {
    /// Serialize this instance to `out` in git `packetline` format, returning the amount of bytes written to `out`.
    pub fn to_write(&self, out: impl io::Write) -> Result<usize, encode::Error> {
        match self {
            Borrowed::Data(d) => encode::data_to_write(d, out),
            Borrowed::Flush => encode::flush_to_write(out).map_err(Into::into),
            Borrowed::Delimiter => encode::delim_to_write(out).map_err(Into::into),
            Borrowed::ResponseEnd => encode::response_end_to_write(out).map_err(Into::into),
        }
    }

    /// Return this instance as slice if it's [`Data`][Borrowed::Data].
    #[must_use]
    pub fn as_slice(&self) -> Option<&[u8]> {
        match self {
            Borrowed::Data(d) => Some(d),
            Borrowed::Flush | Borrowed::Delimiter | Borrowed::ResponseEnd => None,
        }
    }
    /// Return this instance's [`as_slice()`][Borrowed::as_slice()] as [`BStr`].
    pub fn as_bstr(&self) -> Option<&BStr> {
        self.as_slice().map(Into::into)
    }
    /// Interpret this instance's [`as_slice()`][Borrowed::as_slice()] as [`Error`].
    ///
    /// This works for any data received in an error [channel][crate::Channel].
    ///
    /// Note that this creates an unchecked error using the slice verbatim, which is useful to [serialize it][Error::to_write()].
    /// See [`check_error()`][Borrowed::check_error()] for a version that assures the error information is in the expected format.
    pub fn to_error(&self) -> Option<Error<'_>> {
        self.as_slice().map(Error)
    }
    /// Check this instance's [`as_slice()`][Borrowed::as_slice()] is a valid [`Error`] and return it.
    ///
    /// This works for any data received in an error [channel][crate::Channel].
    #[must_use]
    pub fn check_error(&self) -> Option<Error<'_>> {
        self.as_slice().and_then(|data| {
            if data.len() >= ERR_PREFIX.len() && &data[..ERR_PREFIX.len()] == ERR_PREFIX {
                Some(Error(&data[ERR_PREFIX.len()..]))
            } else {
                None
            }
        })
    }
    /// Return this instance as text, with the trailing newline truncated if present.
    pub fn to_text(&self) -> Option<Text<'_>> {
        self.as_slice().map(Into::into)
    }

    /// Interpret the data in this [`slice`][Borrowed::as_slice()] as [`Band`] according to the given `kind` of channel.
    ///
    /// Note that this is only relevant in a side-band channel.
    /// See [`decode_band()`][Borrowed::decode_band()] in case `kind` is unknown.
    #[must_use]
    pub fn to_band(&self, kind: Channel) -> Option<Band<'_>> {
        self.as_slice().map(|d| match kind {
            Channel::Data => Band::Data(d),
            Channel::Progress => Band::Progress(d),
            Channel::Error => Band::Error(d),
        })
    }

    /// Decode the band of this [`slice`][Borrowed::as_slice()], or panic if it is not actually a side-band line.
    pub fn decode_band(&self) -> Result<Band<'_>, DecodeBandError> {
        let d = self.as_slice().ok_or(DecodeBandError::NonDataLine)?;
        Ok(match d[0] {
            1 => Band::Data(&d[1..]),
            2 => Band::Progress(&d[1..]),
            3 => Band::Error(&d[1..]),
            band => return Err(DecodeBandError::InvalidSideBand(band)),
        })
    }
}

use quick_error::quick_error;
quick_error! {
    /// The error used in [`decode_band()`][Borrowed::decode_band()].
    #[derive(Debug)]
    #[allow(missing_docs)]
    pub enum DecodeBandError {
        InvalidSideBand(band: u8) {
            display("attempt to decode a non-side channel line or input was malformed: {}", band)
        }
        NonDataLine {
            display("attempt to decode a non-data line into a side-channel band")
        }
    }
}

/// A packet line representing an Error in a side-band channel.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Error<'a>(pub &'a [u8]);

impl<'a> Error<'a> {
    /// Serialize this line as error to `out`.
    ///
    /// This includes a marker to allow decoding it outside of a side-band channel, returning the amount of bytes written.
    pub fn to_write(&self, out: impl io::Write) -> Result<usize, encode::Error> {
        encode::error_to_write(self.0, out)
    }
}

/// A packet line representing text, which may include a trailing newline.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Text<'a>(pub &'a [u8]);

impl<'a> From<&'a [u8]> for Text<'a> {
    fn from(d: &'a [u8]) -> Self {
        let d = if d[d.len() - 1] == b'\n' { &d[..d.len() - 1] } else { d };
        Text(d)
    }
}

impl<'a> Text<'a> {
    /// Return this instance's data.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.0
    }
    /// Return this instance's data as [`BStr`].
    #[must_use]
    pub fn as_bstr(&self) -> &BStr {
        self.0.into()
    }
    /// Serialize this instance to `out`, appending a newline if there is none, returning the amount of bytes written.
    pub fn to_write(&self, out: impl io::Write) -> Result<usize, encode::Error> {
        encode::text_to_write(self.0, out)
    }
}

/// A band in a side-band channel.
#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub enum Band<'a> {
    /// A band carrying data.
    Data(&'a [u8]),
    /// A band carrying user readable progress information.
    Progress(&'a [u8]),
    /// A band carrying user readable errors.
    Error(&'a [u8]),
}

impl<'a> Band<'a> {
    /// Serialize this instance to `out`, returning the amount of bytes written.
    ///
    /// The data written to `out` can be decoded with [`Borrowed::decode_band()]`.
    pub fn to_write(&self, out: impl io::Write) -> Result<usize, encode::Error> {
        match self {
            Band::Data(d) => encode::band_to_write(Channel::Data, d, out),
            Band::Progress(d) => encode::band_to_write(Channel::Progress, d, out),
            Band::Error(d) => encode::band_to_write(Channel::Error, d, out),
        }
    }
}
