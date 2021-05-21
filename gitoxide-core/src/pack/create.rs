use git_hash::ObjectId;
use git_object::bstr::ByteVec;
use git_odb::{linked, pack, FindExt};
use std::{ffi::OsStr, io, path::Path, str::FromStr, sync::Arc};

#[derive(PartialEq, Debug)]
pub enum ObjectExpansion {
    None,
    TreeTraversal,
    TreeDiff,
}

impl ObjectExpansion {
    pub fn variants() -> &'static [&'static str] {
        &["none", "tree-traversal", "tree-diff"]
    }
}

impl Default for ObjectExpansion {
    fn default() -> Self {
        ObjectExpansion::None
    }
}

impl FromStr for ObjectExpansion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use ObjectExpansion::*;
        let slc = s.to_ascii_lowercase();
        Ok(match slc.as_str() {
            "none" => None,
            "tree-traversal" => TreeTraversal,
            "tree-diff" => TreeDiff,
            _ => return Err("invalid value".into()),
        })
    }
}

impl From<ObjectExpansion> for pack::data::output::count_objects::ObjectExpansion {
    fn from(v: ObjectExpansion) -> Self {
        use pack::data::output::count_objects::ObjectExpansion::*;
        match v {
            ObjectExpansion::None => AsIs,
            ObjectExpansion::TreeTraversal => TreeContents,
            ObjectExpansion::TreeDiff => TreeAdditionsComparedToAncestor,
        }
    }
}

/// A general purpose context for many operations provided here
pub struct Context {
    /// The way input objects should be handled
    pub expansion: ObjectExpansion,
    /// If set, don't use more than this amount of threads.
    /// Otherwise, usually use as many threads as there are logical cores.
    /// A value of 0 is interpreted as no-limit
    pub thread_limit: Option<usize>,
}

pub fn create(
    repository: impl AsRef<Path>,
    tips: impl IntoIterator<Item = impl AsRef<OsStr>>,
    input: Option<impl io::BufRead + Send + 'static>,
    out: impl io::Write,
    ctx: Context,
) -> anyhow::Result<()> {
    let db = Arc::new(find_db(repository)?);
    let tips = tips.into_iter();
    let input: Box<dyn Iterator<Item = ObjectId> + Send + 'static> = match input {
        None => Box::new(
            git_traverse::commit::Ancestors::new(
                tips.map(|t| git_hash::ObjectId::from_hex(&Vec::from_os_str_lossy(t.as_ref())))
                    .collect::<Result<Vec<_>, _>>()?,
                git_traverse::commit::ancestors::State::default(),
                {
                    let db = Arc::clone(&db);
                    move |oid, buf| db.find_existing_commit_iter(oid, buf, &mut pack::cache::Never).ok()
                },
            )
            .filter_map(Result::ok),
        ),
        Some(input) => Box::new(input.lines().filter_map(|hex_id| {
            hex_id
                .ok()
                .and_then(|hex_id| git_hash::ObjectId::from_hex(hex_id.as_bytes()).ok())
        })),
    };

    let chunk_size = 200;
    let counts = {
        let counts_iter = pack::data::output::count_objects_iter(
            Arc::clone(&db),
            pack::cache::lru::StaticLinkedList::<64>::default,
            input,
            git_features::progress::Discard,
            pack::data::output::count_objects::Options {
                thread_limit: ctx.thread_limit,
                chunk_size,
                input_object_expansion: ctx.expansion.into(),
            },
        );
        let mut counts = Vec::new();
        for c in counts_iter {
            counts.extend(c?.into_iter());
        }
        counts
    };
    let num_objects = counts.len();
    let entries = pack::data::output::objects_to_entries_iter(
        counts,
        Arc::clone(&db),
        pack::cache::lru::StaticLinkedList::<64>::default,
        git_features::progress::Discard,
        pack::data::output::objects_to_entries::Options {
            thread_limit: ctx.thread_limit,
            chunk_size,
            version: Default::default(),
        },
    );
    let mut output_iter = pack::data::output::EntriesToBytesIter::new(
        entries,
        out,
        num_objects as u32,
        pack::data::Version::default(),
        git_hash::Kind::default(),
    );
    while let Some(io_res) = output_iter.next() {
        let _written = io_res?;
    }
    output_iter.into_write().flush()?;
    Ok(())
}

fn find_db(repository: impl AsRef<Path>) -> anyhow::Result<linked::Db> {
    let path = repository.as_ref();
    Ok(linked::Db::at(path.join(".git").join("objects"))?)
}