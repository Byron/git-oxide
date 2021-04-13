use crate::{alternate, compound, linked};
use std::path::PathBuf;

/// The error returned by [`linked::Db::at()`]
#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error(transparent)]
    CompoundDbInit(#[from] compound::init::Error),
    #[error(transparent)]
    AlternateResolve(#[from] alternate::Error),
}

impl linked::Db {
    #[allow(missing_docs)]
    pub fn at(objects_directory: impl Into<PathBuf>) -> Result<Self, Error> {
        let mut dbs = vec![compound::Db::at(objects_directory.into())?];
        for object_path in alternate::resolve(dbs[0].loose.path.clone())?.into_iter() {
            dbs.push(compound::Db::at(object_path)?);
        }
        Ok(linked::Db { dbs })
    }
}

impl std::convert::TryFrom<PathBuf> for linked::Db {
    type Error = Error;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        linked::Db::at(value)
    }
}