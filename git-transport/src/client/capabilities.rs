use crate::Protocol;
use bstr::{BStr, BString, ByteSlice};
use quick_error::quick_error;
use std::io;

quick_error! {
    /// The error used in [`Capabilities::from_bytes()`] and [`Capabilities::from_lines()`].
    #[derive(Debug)]
    #[allow(missing_docs)]
    pub enum Error {
        MissingDelimitingNullByte {
            display("Capabilities were missing entirely as there was no 0 byte")
        }
        NoCapabilities {
            display("there was not a single capability behind the delimiter")
        }
        MissingVersionLine {
            display("a version line was expected, but none was retrieved")
        }
        MalformattedVersionLine(actual: String) {
            display("expected 'version X', got '{}'", actual)
        }
        UnsupportedVersion(wanted: Protocol, got: String) {
            display("Got unsupported version '{}', expected '{}'", got, *wanted as usize)
        }
        Io(err: io::Error) {
            display("An IO error occurred while reading V2 lines")
            from()
            source(err)
        }
    }
}

/// A structure to represent multiple [capabilities][Capability] or features supported by the server.
#[derive(Debug, Clone)]
pub struct Capabilities {
    data: BString,
    value_sep: u8,
}

/// The name of a single capability.
pub struct Capability<'a>(&'a BStr);

impl<'a> Capability<'a> {
    /// Returns the name of the capability.
    ///
    /// Most capabilities only consist of a name, making them appear like a feature toggle.
    #[must_use]
    pub fn name(&self) -> &BStr {
        self.0
            .splitn(2, |b| *b == b'=')
            .next()
            .expect("there is always a single item")
            .as_bstr()
    }
    /// Returns the value associated with the capability.
    ///
    /// Note that the caller must know whether a single or multiple values are expected, in which
    /// case [`values()`][Capability::values()] should be called.
    #[must_use]
    pub fn value(&self) -> Option<&BStr> {
        self.0.splitn(2, |b| *b == b'=').nth(1).map(|s| s.as_bstr())
    }
    /// Returns the values of a capability if its [`value()`][Capability::value()] is space separated.
    #[must_use]
    pub fn values(&self) -> Option<impl Iterator<Item = &BStr>> {
        self.value().map(|v| v.split(|b| *b == b' ').map(|s| s.as_bstr()))
    }
}

impl Capabilities {
    /// Parse capabilities from the given `bytes`.
    ///
    /// Useful in case they are encoded within a `ref` behind a null byte.
    pub fn from_bytes(bytes: &[u8]) -> Result<(Self, usize), Error> {
        let delimiter_pos = bytes.find_byte(0).ok_or(Error::MissingDelimitingNullByte)?;
        if delimiter_pos + 1 == bytes.len() {
            return Err(Error::NoCapabilities);
        }
        let capabilities = &bytes[delimiter_pos + 1..];
        Ok((
            Self {
                data: capabilities.as_bstr().to_owned(),
                value_sep: b' ',
            },
            delimiter_pos,
        ))
    }
    /// Parse capabilities from the given `read`.
    ///
    /// Useful for parsing capabilities from a data sent from a server.
    pub fn from_lines(read: impl io::BufRead) -> Result<Self, Error> {
        let mut lines = read.lines();
        let version_line = lines.next().ok_or(Error::MissingVersionLine)??;
        let (name, value) = version_line.split_at(
            version_line
                .find(' ')
                .ok_or_else(|| Error::MalformattedVersionLine(version_line.clone()))?,
        );
        if name != "version" {
            return Err(Error::MalformattedVersionLine(version_line));
        }
        if value != " 2" {
            return Err(Error::UnsupportedVersion(Protocol::V2, value.to_owned()));
        }
        Ok(Self {
            value_sep: b'\n',
            data: lines
                .inspect(|l| {
                    if let Ok(l) = l {
                        assert!(
                            !l.contains('\n'),
                            "newlines are not expected in keys or values, got '{}'",
                            l
                        )
                    }
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n")
                .into(),
        })
    }

    /// Returns true of the given `feature` is mentioned in this list of capabilities.
    #[must_use]
    pub fn contains(&self, feature: &str) -> bool {
        self.capability(feature).is_some()
    }

    /// Returns the capability with `name`.
    #[must_use]
    pub fn capability(&self, name: &str) -> Option<Capability<'_>> {
        self.iter().find(|c| c.name() == name.as_bytes().as_bstr())
    }

    /// Returns an iterator over all capabilities.
    pub fn iter(&self) -> impl Iterator<Item = Capability<'_>> {
        self.data
            .split(move |b| *b == self.value_sep)
            .map(|c| Capability(c.as_bstr()))
    }
}

pub(crate) mod recv {
    use crate::{client, client::Capabilities, Protocol};
    use bstr::ByteSlice;
    use std::io;

    pub struct Outcome<'a> {
        pub capabilities: Capabilities,
        pub refs: Option<Box<dyn io::BufRead + 'a>>,
        pub protocol: Protocol,
    }

    pub fn v1_or_v2_as_detected<T: io::Read>(
        rd: &mut git_packetline::Provider<T>,
    ) -> Result<Outcome<'_>, client::Error> {
        // NOTE that this is vitally important - it is turned on and stays on for all following requests so
        // we automatically abort if the server sends an ERR line anywhere.
        // We are sure this can't clash with binary data when sent due to the way the PACK
        // format looks like, thus there is no binary blob that could ever look like an ERR line by accident.
        rd.fail_on_err_lines(true);

        let capabilities_or_version = rd
            .peek_line()
            .ok_or(client::Error::ExpectedLine("capabilities or version"))???;
        let first_line = capabilities_or_version
            .to_text()
            .ok_or(client::Error::ExpectedLine("text"))?;

        let version = if first_line.as_bstr().starts_with_str("version ") {
            if first_line.as_bstr().ends_with_str(" 1") {
                Protocol::V1
            } else {
                Protocol::V2
            }
        } else {
            Protocol::V1
        };
        match version {
            Protocol::V1 => {
                let (capabilities, delimiter_position) = Capabilities::from_bytes(first_line.0)?;
                rd.peek_buffer_replace_and_truncate(delimiter_position, b'\n');
                Ok(Outcome {
                    capabilities,
                    refs: Some(Box::new(rd.as_read())),
                    protocol: Protocol::V1,
                })
            }
            Protocol::V2 => Ok(Outcome {
                capabilities: Capabilities::from_lines(rd.as_read())?,
                refs: None,
                protocol: Protocol::V2,
            }),
        }
    }
}
