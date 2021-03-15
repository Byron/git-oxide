use crate::{remote::refs::JsonRef, OutputFormat, Protocol};
use git_features::progress::Progress;
use git_object::{
    bstr::{BString, ByteSlice},
    owned,
};
use git_odb::pack;
use git_protocol::{
    fetch::{Action, Arguments, Ref, Response},
    git_transport::{self, client::Capabilities},
};
use std::{
    io::{self, BufRead},
    path::PathBuf,
};

pub const PROGRESS_RANGE: std::ops::RangeInclusive<u8> = 1..=3;

pub struct Context<W: io::Write> {
    pub thread_limit: Option<usize>,
    pub format: OutputFormat,
    pub out: W,
}

struct CloneDelegate<W: io::Write> {
    ctx: Context<W>,
    directory: Option<PathBuf>,
    refs_directory: Option<PathBuf>,
    ref_filter: Option<&'static [&'static str]>,
}
static FILTER: &[&str] = &["HEAD", "refs/tags", "refs/heads"];

impl<W: io::Write> git_protocol::fetch::Delegate for CloneDelegate<W> {
    fn prepare_ls_refs(
        &mut self,
        server: &Capabilities,
        arguments: &mut Vec<BString>,
        _features: &mut Vec<(&str, Option<&str>)>,
    ) {
        if server.contains("ls-refs") {
            arguments.extend(FILTER.iter().map(|r| format!("ref-prefix {}", r).into()));
        }
    }

    fn prepare_fetch(
        &mut self,
        version: git_transport::Protocol,
        _server: &Capabilities,
        _features: &mut Vec<(&str, Option<&str>)>,
        _refs: &[Ref],
    ) -> Action {
        if version == git_transport::Protocol::V1 {
            self.ref_filter = Some(FILTER);
        }
        Action::Continue
    }

    fn negotiate(&mut self, refs: &[Ref], arguments: &mut Arguments, _previous: Option<&Response>) -> Action {
        for r in refs {
            let (path, id) = r.unpack();
            match self.ref_filter {
                Some(ref_prefixes) => {
                    if ref_prefixes.iter().any(|prefix| path.starts_with_str(prefix)) {
                        arguments.want(id.to_borrowed());
                    }
                }
                None => arguments.want(id.to_borrowed()),
            }
        }
        Action::Close
    }

    fn receive_pack(
        &mut self,
        input: impl BufRead,
        progress: impl Progress,
        refs: &[Ref],
        _previous: &Response,
    ) -> io::Result<()> {
        let options = pack::bundle::write::Options {
            thread_limit: self.ctx.thread_limit,
            index_kind: pack::index::Version::V2,
            iteration_mode: pack::data::iter::Mode::Verify,
        };
        let outcome = pack::bundle::Bundle::write_stream_to_directory(input, self.directory.take(), progress, options)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        if let Some(directory) = self.refs_directory.take() {
            let assure_dir = |path: &git_object::bstr::BString| {
                assert!(!path.starts_with_str("/"), "no ref start with a /, they are relative");
                let path = directory.join(path.to_path_lossy());
                std::fs::create_dir_all(path.parent().expect("multi-component path")).map(|_| path)
            };
            for r in refs {
                let (path, content) = match r {
                    Ref::Symbolic { path, target, .. } => (assure_dir(path)?, format!("ref: {}", target)),
                    Ref::Peeled { path, tag: object, .. } | Ref::Direct { path, object } => {
                        (assure_dir(path)?, object.to_string())
                    }
                };
                std::fs::write(path, content.as_bytes())?;
            }
        }

        match self.ctx.format {
            OutputFormat::Human => drop(print(&mut self.ctx.out, outcome, refs)),
            #[cfg(feature = "serde1")]
            OutputFormat::Json => {
                serde_json::to_writer_pretty(&mut self.ctx.out, &JSONOutcome::from_outcome_and_refs(outcome, refs))?
            }
        };
        Ok(())
    }
}

#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct JSONBundleWriteOutcome {
    pub index_kind: pack::index::Version,
    pub index_hash: String,

    pub data_hash: String,
    pub num_objects: u32,
}

impl From<pack::index::write::Outcome> for JSONBundleWriteOutcome {
    fn from(v: pack::index::write::Outcome) -> Self {
        Self {
            index_kind: v.index_kind,
            num_objects: v.num_objects,
            data_hash: v.data_hash.to_string(),
            index_hash: v.index_hash.to_string(),
        }
    }
}

#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct JSONOutcome {
    pub index: JSONBundleWriteOutcome,
    pub pack_kind: pack::data::Version,

    pub index_path: Option<PathBuf>,
    pub data_path: Option<PathBuf>,

    pub refs: Vec<JsonRef>,
}

impl JSONOutcome {
    #[must_use]
    pub fn from_outcome_and_refs(v: pack::bundle::write::Outcome, refs: &[Ref]) -> Self {
        Self {
            index: v.index.into(),
            pack_kind: v.pack_kind,
            index_path: v.index_path,
            data_path: v.data_path,
            refs: refs.iter().cloned().map(Into::into).collect(),
        }
    }
}

fn print_hash_and_path(out: &mut impl io::Write, name: &str, id: owned::Id, path: Option<PathBuf>) -> io::Result<()> {
    match path {
        Some(path) => writeln!(out, "{}: {} ({})", name, id, path.display()),
        None => writeln!(out, "{}: {}", name, id),
    }
}

fn print(out: &mut impl io::Write, res: pack::bundle::write::Outcome, refs: &[Ref]) -> io::Result<()> {
    print_hash_and_path(out, "index", res.index.index_hash, res.index_path)?;
    print_hash_and_path(out, "pack", res.index.data_hash, res.data_path)?;
    writeln!(out)?;
    crate::remote::refs::print(out, refs)?;
    Ok(())
}

pub fn receive<P: Progress, W: io::Write>(
    protocol: Option<Protocol>,
    url: &str,
    directory: Option<PathBuf>,
    refs_directory: Option<PathBuf>,
    progress: P,
    ctx: Context<W>,
) -> anyhow::Result<()> {
    let transport = git_protocol::git_transport::client::connect(url.as_bytes(), protocol.unwrap_or_default().into())?;
    let mut delegate = CloneDelegate {
        ctx,
        directory,
        refs_directory,
        ref_filter: None,
    };
    git_protocol::fetch(transport, &mut delegate, git_protocol::credentials::helper, progress)?;
    Ok(())
}
