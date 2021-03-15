use crate::{MAX_DATA_LEN, U16_HEX_BYTES};
use std::io;

/// An implementor of [`Write`][io::Write] which passes all input to an inner `Write` in packet line data encoding,
/// one line per `write(…)` call or as many lines as it takes if the data doesn't fit into the maximum allowed line length.
pub struct Writer<T> {
    /// the `Write` implementation to which to propagate packet lines
    pub inner: T,
    binary: bool,
}

impl<T: io::Write> Writer<T> {
    /// Create a new instance from the given `write`
    pub fn new(write: T) -> Self {
        Self {
            inner: write,
            binary: true,
        }
    }
    /// If called, each call to [`write()`][io::Write::write()] will write bytes as is.
    pub fn enable_binary_mode(&mut self) {
        self.binary = true;
    }
    /// If called, each call to [`write()`][io::Write::write()] will write the input as text, appending a trailing newline
    /// if needed before writing.
    pub fn enable_text_mode(&mut self) {
        self.binary = false;
    }
    /// As [`enable_text_mode()`][Writer::enable_text_mode()], but suitable for chaining.
    pub fn text_mode(mut self) -> Self {
        self.binary = false;
        self
    }
    /// As [`enable_binary_mode()`][Writer::enable_binary_mode()], but suitable for chaining.
    pub fn binary_mode(mut self) -> Self {
        self.binary = true;
        self
    }
}

impl<T: io::Write> io::Write for Writer<T> {
    fn write(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "empty packet lines are not permitted as '0004' is invalid",
            ));
        }

        let mut written = 0;
        while !buf.is_empty() {
            let (data, rest) = buf.split_at(buf.len().min(MAX_DATA_LEN));
            written += if self.binary {
                crate::encode::data_to_write(data, &mut self.inner)
            } else {
                crate::encode::text_to_write(data, &mut self.inner)
            }
            .map_err(|err| {
                use crate::encode::Error::{DataIsEmpty, DataLengthLimitExceeded, Io};
                match err {
                    Io(err) => err,
                    DataIsEmpty | DataLengthLimitExceeded(_) => {
                        unreachable!("We are handling empty and large data here, so this can't ever happen")
                    }
                }
            })?;
            // subtract header (and trailng NL) because write-all can't handle writing more than it passes in
            written -= U16_HEX_BYTES + if self.binary { 0 } else { 1 };
            buf = rest;
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
