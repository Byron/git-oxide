use git_object::owned;
use std::path::PathBuf;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

#[cfg(not(windows))]
fn fixup(v: Vec<u8>) -> Vec<u8> {
    v
}

#[cfg(windows)]
fn fixup(v: Vec<u8>) -> Vec<u8> {
    // Git checks out text files with line ending conversions, git itself will of course not put '\r\n' anywhere,
    // so that wouldn't be expected in an object and doesn't have to be parsed.
    use bstr::ByteSlice;
    v.replace(b"\r\n", "\n")
}

#[must_use]
pub fn hex_to_id(hex: &str) -> owned::Id {
    owned::Id::from_40_bytes_in_hex(hex.as_bytes()).expect("40 bytes hex")
}

#[must_use]
pub fn fixture_path(path: &str) -> PathBuf {
    PathBuf::from("tests").join("fixtures").join(path)
}

mod alternate;
mod loose;
mod pack;
mod sink;
