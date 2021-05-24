#![deny(missing_docs, unsafe_code, rust_2018_idioms)]

//! Git stores all of its data as _Objects_, which are nothing more than data along with a hash over all data. Thus it's an
//! object store indexed by data with inherent deduplication: the same data will have the same hash, and thus occupy the same
//! space within the database.
//!
//! There are various flavours of object databases, all of which supporting iteration, reading and possibly writing.
//!
//! * [`store::loose::Backend`]
//!   * A database storing one object per file, named by its hash, using zlib compression.
//!   * O(1) reads and writes, bound by IO operations per second
//! * [`pack::Bundle`]
//!   * A database storing multiple objects within an indexed pack file, reaching compression ratios of 60 to 1 or more.
//!   * Slow writes and fast reads
//! * [`store::compound::Backend`]
//!   * A database using a [`store::loose::Backend`] for writes and multiple [`pack::Bundle`]s for object reading. It can also refer to multiple
//!     additional [`store::compound::Backend`] instances using git-alternates.
//!   * This is the database closely resembling the object database in a git repository, and probably what most people would want to use.
//! * [`store::linked::Db`]
//!   * A database containing various [`compound::Backends`][store::compound::Backend] as gathered from `alternates` files.

pub use find::{Find, FindExt};
pub use write::Write;

mod zlib;

///
pub mod store;

pub mod pack;

pub(crate) mod hash;

pub mod data;

///
pub mod find;
///
pub mod write;
