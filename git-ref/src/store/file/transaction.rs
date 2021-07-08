use crate::{
    mutable::Target,
    store::file,
    transaction::{Change, RefEdit, RefEditsExt, RefLog},
};
use bstr::BString;
use std::io::Write;

#[derive(Copy, Clone, Debug)]
enum Index {
    Parent(usize),
    Child(usize),
}

impl Index {
    fn parent_index(&self) -> usize {
        match self {
            Index::Parent(idx) => *idx,
            Index::Child(_) => unreachable!("caller made sure this can't happen"),
        }
    }
}

#[derive(Debug)]
struct Edit {
    update: RefEdit,
    lock: Option<git_lock::Marker>,
    /// Set if this update is coming from a symbolic reference and used to make it appear like it is the one that is handled,
    /// instead of the referent reference.
    index: Option<Index>,
}

impl Edit {
    fn name(&self) -> BString {
        self.update.name.0.clone()
    }
}

impl std::borrow::Borrow<RefEdit> for Edit {
    fn borrow(&self) -> &RefEdit {
        &self.update
    }
}

impl std::borrow::BorrowMut<RefEdit> for Edit {
    fn borrow_mut(&mut self) -> &mut RefEdit {
        &mut self.update
    }
}
/// A transaction
pub struct Transaction<'a> {
    store: &'a file::Store,
    updates: Vec<Edit>,
    state: State,
    lock_fail_mode: git_lock::acquire::Fail,
}

impl<'a> Transaction<'a> {
    /// Find referents with parent links (from the back) as these have been pushed there during splitting. Then lookup its parent links
    /// and point them to the children instead. This allows reflogs to be written so that the old oid is the peeled one, instead of
    /// the actual reference value of the symbolic ref.
    ///
    /// This means later we can traverse down the chain for this lookup, but we can't traverse upwards anymore for instance to display
    /// the original name instead of the name of the split referents names. This is intentional as it seems confusing to report the 'wrong'
    /// ref name on error. Maybe that's something to change once its clear how these error messages should look like.
    fn invert_parent_links(changes: &mut Vec<Edit>) {
        if changes.is_empty() {
            return;
        }
        let mut leaf_cursor_index = changes.len();
        loop {
            let leaf = changes[..leaf_cursor_index]
                .iter()
                .enumerate()
                .rfind(|(_cidx, c)| matches!(c.index, Some(Index::Parent(_))))
                .map(|(cidx, c)| (c.index.expect("an index").parent_index(), cidx));
            match leaf {
                Some((parent_idx, child_index)) => {
                    leaf_cursor_index = child_index;
                    let mut next_child_index = child_index;
                    let mut parent_idx_cursor = Some(parent_idx);
                    while let Some((pidx, parent)) = parent_idx_cursor.take().map(|idx| (idx, &mut changes[idx])) {
                        match parent.index {
                            Some(Index::Child(_)) => unreachable!("there is only one path to a child"),
                            Some(Index::Parent(next_parent)) => {
                                parent.index = Some(Index::Child(next_child_index));
                                next_child_index = pidx;
                                parent_idx_cursor = Some(next_parent);
                            }
                            None => {}
                        }
                    }
                }
                None => break,
            }
        }
        dbg!(changes);
    }

    fn lock_ref_and_apply_change(
        store: &file::Store,
        lock_fail_mode: git_lock::acquire::Fail,
        change: &mut Edit,
    ) -> Result<(), Error> {
        assert!(
            change.lock.is_none(),
            "locks can only be acquired once and it's all or nothing"
        );

        let relative_path = change.update.name.to_path();
        let existing_ref = store
            .ref_contents(relative_path.as_ref())
            .map_err(Error::from)
            .and_then(|opt| {
                opt.map(|buf| file::Reference::try_from_path(store, relative_path.as_ref(), &buf).map_err(Error::from))
                    .transpose()
            })
            .or_else(|err| match err {
                Error::ReferenceDecode(_) => Ok(None),
                other => Err(other),
            });
        let lock = match &mut change.update.change {
            Change::Delete { previous, .. } => {
                let lock = git_lock::Marker::acquire_to_hold_resource(
                    store.ref_path(&relative_path),
                    lock_fail_mode,
                    Some(store.base.to_owned()),
                )?;
                let existing_ref = existing_ref?;
                match (&previous, &existing_ref) {
                    (None, None | Some(_)) => {}
                    (Some(_previous), None) => {
                        return Err(Error::DeleteReferenceMustExist {
                            full_name: change.name(),
                        })
                    }
                    (Some(previous), Some(existing)) => {
                        if !previous.is_null() && *previous != existing.target() {
                            let expected = previous.clone();
                            return Err(Error::DeleteReferenceOutOfDate {
                                full_name: change.name(),
                                expected,
                                actual: existing.target().to_owned(),
                            });
                        }
                    }
                }

                // Keep the previous value for the caller and ourselves. Maybe they want to keep a log of sorts.
                if let Some(existing) = existing_ref {
                    *previous = Some(existing.target().into());
                }

                lock
            }
            Change::Update { previous, new, .. } => {
                let mut lock = git_lock::File::acquire_to_update_resource(
                    store.ref_path(&relative_path),
                    lock_fail_mode,
                    Some(store.base.to_owned()),
                )?;

                if let Some(_expected_target) = previous {
                    todo!("check previous value, if object id is not null");
                }

                if let Some(existing) = existing_ref? {
                    *previous = Some(existing.target().into());
                }

                lock.with_mut(|file| match new {
                    Target::Peeled(oid) => file.write_all(oid.as_bytes()),
                    Target::Symbolic(name) => file.write_all(b"ref: ").and_then(|_| file.write_all(name.as_ref())),
                })?;

                lock.close()?
            }
        };
        change.lock = Some(lock);
        Ok(())
    }
}

impl<'a> Transaction<'a> {
    /// Discard the transaction and re-obtain the initial edits
    pub fn into_edits(self) -> Vec<RefEdit> {
        self.updates.into_iter().map(|e| e.update).collect()
    }

    /// Prepare for calling [`commit(…)`][Transaction::commit()] in a way that can be rolled back perfectly.
    ///
    /// If the operation succeeds, the transaction can be committed or dropped to cause a rollback automatically.
    /// Rollbacks happen automatically on failure and they tend to be perfect.
    /// This method is idempotent.
    pub fn prepare(mut self) -> Result<Self, Error> {
        Ok(match self.state {
            State::Prepared => self,
            State::Open => {
                self.updates
                    .pre_process(self.store, |idx, update| Edit {
                        update,
                        lock: None,
                        index: Some(Index::Parent(idx)),
                    })
                    .map_err(Error::PreprocessingFailed)?;

                for change in self.updates.iter_mut() {
                    Self::lock_ref_and_apply_change(self.store, self.lock_fail_mode, change)?;
                }
                Self::invert_parent_links(&mut self.updates);
                self.state = State::Prepared;
                self
            }
        })
    }

    /// Make all [prepared][Transaction::prepare()] permanent and return the performed edits which represent the current
    /// state of the affected refs in the ref store in that instant. Please note that the obtained edits may have been
    /// adjusted to contain more dependent edits or additional information.
    ///
    /// On error the transaction may have been performed partially, depending on the nature of the error, and no attempt to roll back
    /// partial changes is made.
    ///
    /// In this stage, we perform the following operations:
    ///
    /// * write the ref log
    /// * move updated refs into place
    /// * delete reflogs
    /// * delete their corresponding reference (if applicable)
    ///   along with empty parent directories
    ///
    /// Note that transactions will be prepared automatically as needed.
    pub fn commit(mut self) -> Result<Vec<RefEdit>, Error> {
        match self.state {
            State::Open => self.prepare()?.commit(),
            State::Prepared => {
                // Perform updates first so live commits remain referenced
                for change in self.updates.iter_mut() {
                    assert!(!change.update.deref, "Deref mode is turned into splits and turned off");
                    match &change.update.change {
                        // reflog first, then reference
                        Change::Update {
                            log: _,
                            new,
                            previous: _,
                        } => {
                            let lock = change.lock.take().expect("each ref is locked");
                            match new {
                                Target::Symbolic(_) => {} // look up the leaf/peel id to know what the old oid was
                                Target::Peeled(_oid) => {
                                    // self.store.create_or_append_reflog(&lock, change.)
                                    todo!("commit other reflog write cases")
                                }
                            }
                            lock.commit()?
                        }
                        Change::Delete { .. } => {}
                    }
                }

                for change in self.updates.iter_mut() {
                    match &change.update.change {
                        Change::Update { .. } => {}
                        Change::Delete { mode, .. } => {
                            let lock = change.lock.take().expect("each ref is locked, even deletions");
                            let (rm_reflog, rm_ref) = match mode {
                                RefLog::AndReference => (true, true),
                                RefLog::Only => (true, false),
                            };

                            // Reflog deletion happens first in case it fails a ref without log is less terrible than
                            // a log without a reference.
                            if rm_reflog {
                                let reflog_path = self.store.reflog_path(change.update.name.borrow());
                                if let Err(err) = std::fs::remove_file(reflog_path) {
                                    if err.kind() != std::io::ErrorKind::NotFound {
                                        return Err(Error::DeleteReflog {
                                            err,
                                            full_name: change.name(),
                                        });
                                    }
                                }
                            }
                            if rm_ref {
                                let reference_path = self.store.ref_path(change.update.name.to_path().as_ref());
                                if let Err(err) = std::fs::remove_file(reference_path) {
                                    if err.kind() != std::io::ErrorKind::NotFound {
                                        return Err(Error::DeleteReference {
                                            err,
                                            full_name: change.name(),
                                        });
                                    }
                                }
                            }
                            drop(lock); // allow deletion of empty leading directories
                        }
                    }
                }
                Ok(self.updates.into_iter().map(|edit| edit.update).collect())
            }
        }
    }
}

/// The state of a [`Transaction`]
pub enum State {
    /// The transaction was just created but isn't prepared yet.
    Open,
    /// The transaction is ready to be committed.
    Prepared,
}

/// Edits
impl file::Store {
    /// Open a transaction with the given `edits`, and determine how to fail if a `lock` cannot be obtained.
    pub fn transaction(
        &self,
        edits: impl IntoIterator<Item = RefEdit>,
        lock: git_lock::acquire::Fail,
    ) -> Transaction<'_> {
        Transaction {
            store: self,
            updates: edits
                .into_iter()
                .map(|update| Edit {
                    update,
                    lock: None,
                    index: None,
                })
                .collect(),
            state: State::Open,
            lock_fail_mode: lock,
        }
    }
}

mod error {
    use crate::{mutable::Target, store::file};
    use bstr::BString;
    use quick_error::quick_error;

    quick_error! {
        /// The error returned by various [`Transaction`][super::Transaction] methods.
        #[derive(Debug)]
        #[allow(missing_docs)]
        pub enum Error {
            PreprocessingFailed(err: std::io::Error) {
                display("Edit preprocessing failed with error: {}", err.to_string())
                source(err)
            }
            LockAcquire(err: git_lock::acquire::Error) {
                display("A lock could not be obtained for a resource")
                from()
                source(err)
            }
            Io(err: std::io::Error) {
                display("An IO error occurred while applying an edit")
                from()
                source(err)
            }
            DeleteReferenceMustExist { full_name: BString } {
                display("The reference '{}' for deletion did not exist or could not be parsed", full_name)
            }
            DeleteReferenceOutOfDate { full_name: BString, expected: Target, actual: Target } {
                display("The reference '{}' should have content {}, actual content was {}", full_name, expected, actual)
            }
            DeleteReference{ full_name: BString, err: std::io::Error } {
                display("The reference '{}' could not be deleted", full_name)
                source(err)
            }
            DeleteReflog{ full_name: BString, err: std::io::Error } {
                display("The reflog of reference '{}' could not be deleted", full_name)
                source(err)
            }
            ReferenceDecode(err: file::reference::decode::Error) {
                display("Could not read reference")
                from()
                source(err)
            }
        }
    }
}
pub use error::Error;