// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared post-order recursive deletion.

use std::io;
use std::time::Instant;

use super::delete_backend::DeleteBackend;
use super::delete_budget::DeleteBudget;
use super::delete_work::DeleteWork;
use super::directory_mutation_error::directory_mutation_error;
use crate::LocalDeleteOptions;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::error::LocalFileEffectState;

/// Removes `root` and its descendants in post-order without following directory
/// links.
///
/// The supplied budget options constrain discovered entries, depth, pending
/// path storage, and elapsed time. Each native operation is preceded by a
/// cooperative deadline check; an in-flight filesystem call cannot be
/// interrupted.
///
/// # Parameters
///
/// * `backend` provides filesystem operations and path conversion for this
///   traversal.
/// * `root` is the directory to remove and must identify a directory.
/// * `options` sets resource and deadline limits for this deletion.
/// * `started_at` is the monotonic start time used to measure the deadline.
///
/// # Errors
///
/// Returns a contextual local-file error if `root` is not a directory, a
/// backend operation fails, or a configured resource budget or deadline is
/// exceeded. The error records whether an earlier entry was already removed.
pub(crate) fn remove_directory_tree<B: DeleteBackend>(
    backend: &B,
    root: &B::Path,
    options: LocalDeleteOptions,
    started_at: Instant,
) -> LocalResult<()> {
    let mut changed = false;
    let mut budget = DeleteBudget::new(options, started_at);
    let operation = LocalFileOperation::DeleteDirectory;
    let fail = |path: &B::Path, changed: bool, source: io::Error| {
        let error = directory_mutation_error(operation, backend.path(path), changed, source);
        if changed {
            error
        } else {
            error.with_effect_state(LocalFileEffectState::Unchanged)
        }
    };
    let fail_during_removal = |path: &B::Path, changed: bool, source: io::Error| {
        let error = directory_mutation_error(operation, backend.path(path), changed, source);
        if changed {
            error
        } else {
            error.with_effect_state(LocalFileEffectState::Indeterminate)
        }
    };

    budget
        .discover(0)
        .and_then(|()| budget.reserve_path(backend.path(root)))
        .map_err(|error| fail(root, false, error))?;
    let mut work = vec![(DeleteWork::Inspect(root.clone()), 0_usize)];
    while let Some((item, depth)) = work.pop() {
        let path = match &item {
            DeleteWork::Inspect(path) | DeleteWork::RemoveDirectory(path) => path,
        };
        budget.release_path(backend.path(path));
        budget.check_deadline().map_err(|error| fail(path, changed, error))?;
        match item {
            DeleteWork::Inspect(path) => {
                let metadata = backend.metadata(&path).map_err(|error| fail(&path, changed, error))?;
                let is_directory = backend.is_directory(&metadata);
                if depth == 0 && !is_directory {
                    return Err(fail(&path, changed, io::Error::from(io::ErrorKind::NotADirectory)));
                }
                if is_directory {
                    budget.check_deadline().map_err(|error| fail(&path, changed, error))?;
                    let mut reader = backend
                        .open_directory(&path)
                        .map_err(|error| fail(&path, changed, error))?;
                    budget
                        .reserve_path(backend.path(&path))
                        .map_err(|error| fail(&path, changed, error))?;
                    work.push((DeleteWork::RemoveDirectory(path.clone()), depth));
                    let children_start = work.len();
                    loop {
                        budget.check_deadline().map_err(|error| fail(&path, changed, error))?;
                        let Some(child) = backend
                            .next_child(&path, &mut reader)
                            .map_err(|error| fail(&path, changed, error))?
                        else {
                            break;
                        };
                        budget
                            .discover(depth + 1)
                            .and_then(|()| budget.reserve_path(backend.path(&child)))
                            .map_err(|error| fail(&child, changed, error))?;
                        work.push((DeleteWork::Inspect(child), depth + 1));
                    }
                    work[children_start..].reverse();
                } else {
                    budget.check_deadline().map_err(|error| fail(&path, changed, error))?;
                    backend
                        .before_remove(&path)
                        .map_err(|error| fail(&path, changed, error))?;
                    backend
                        .remove_non_directory(&path, &metadata)
                        .map_err(|error| fail_during_removal(&path, changed, error))?;
                    changed = true;
                }
            }
            DeleteWork::RemoveDirectory(path) => {
                backend
                    .before_remove(&path)
                    .map_err(|error| fail(&path, changed, error))?;
                backend
                    .remove_empty_directory(&path)
                    .map_err(|error| fail_during_removal(&path, changed, error))?;
                changed = true;
            }
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::cell::Cell;
    use std::io;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::Instant;

    use super::remove_directory_tree;
    use crate::LocalDeleteOptions;
    use crate::local::internal::delete_backend::DeleteBackend;
    use crate::test_support::install_test_fault;

    struct RecordingBackend {
        root_is_directory: bool,
        child_is_directory: bool,
        yield_child: Cell<bool>,
        open_error: Option<io::ErrorKind>,
        remove_non_directory_error: Option<io::ErrorKind>,
        metadata_calls: Cell<usize>,
        open_calls: Cell<usize>,
        remove_non_directory_calls: Cell<usize>,
        remove_empty_directory_calls: Cell<usize>,
        before_remove_calls: Cell<usize>,
    }

    impl RecordingBackend {
        fn new() -> Self {
            Self {
                root_is_directory: true,
                child_is_directory: true,
                yield_child: Cell::new(false),
                open_error: None,
                remove_non_directory_error: None,
                metadata_calls: Cell::new(0),
                open_calls: Cell::new(0),
                remove_non_directory_calls: Cell::new(0),
                remove_empty_directory_calls: Cell::new(0),
                before_remove_calls: Cell::new(0),
            }
        }
    }

    impl DeleteBackend for RecordingBackend {
        type Path = PathBuf;
        type Metadata = bool;
        type Reader = ();

        fn path<'a>(&self, value: &'a PathBuf) -> &'a Path {
            value
        }

        fn metadata(&self, path: &PathBuf) -> io::Result<bool> {
            self.metadata_calls.set(self.metadata_calls.get() + 1);
            Ok(match path.file_name() {
                Some(name) if name == "child" => self.child_is_directory,
                _ => self.root_is_directory,
            })
        }

        fn is_directory(&self, metadata: &bool) -> bool {
            *metadata
        }

        fn open_directory(&self, _path: &PathBuf) -> io::Result<()> {
            self.open_calls.set(self.open_calls.get() + 1);
            match self.open_error {
                Some(kind) => Err(io::Error::from(kind)),
                None => Ok(()),
            }
        }

        fn next_child(&self, parent: &PathBuf, _reader: &mut ()) -> io::Result<Option<PathBuf>> {
            if self.yield_child.replace(false) {
                Ok(Some(parent.join("child")))
            } else {
                Ok(None)
            }
        }

        fn remove_non_directory(&self, _path: &PathBuf, _metadata: &bool) -> io::Result<()> {
            self.remove_non_directory_calls
                .set(self.remove_non_directory_calls.get() + 1);
            match self.remove_non_directory_error {
                Some(kind) => Err(io::Error::from(kind)),
                None => Ok(()),
            }
        }

        fn remove_empty_directory(&self, _path: &PathBuf) -> io::Result<()> {
            self.remove_empty_directory_calls
                .set(self.remove_empty_directory_calls.get() + 1);
            Ok(())
        }

        fn before_remove(&self, _path: &PathBuf) -> io::Result<()> {
            self.before_remove_calls.set(self.before_remove_calls.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn test_directory_open_is_skipped_when_deadline_expires_after_metadata() {
        let _fault = install_test_fault("local-delete-deadline-3").expect("deadline fault should install");
        let backend = RecordingBackend::new();
        let error = remove_directory_tree(
            &backend,
            &PathBuf::from("tree"),
            LocalDeleteOptions::new().with_recursive(),
            Instant::now(),
        )
        .expect_err("deadline should stop traversal after metadata");

        assert_eq!(error.io_error_kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            error.effect_state(),
            Some(crate::error::LocalFileEffectState::Unchanged)
        );
        assert_eq!(backend.metadata_calls.get(), 1);
        assert_eq!(backend.open_calls.get(), 0);
        assert_eq!(backend.remove_non_directory_calls.get(), 0);
        assert_eq!(backend.remove_empty_directory_calls.get(), 0);
        assert_eq!(backend.before_remove_calls.get(), 0);
    }

    #[test]
    fn test_directory_tree_removes_empty_directory_without_fault() {
        let backend = RecordingBackend::new();
        remove_directory_tree(
            &backend,
            &PathBuf::from("tree"),
            LocalDeleteOptions::new().with_recursive(),
            Instant::now(),
        )
        .expect("empty directory should be removed");

        assert_eq!(backend.metadata_calls.get(), 1);
        assert_eq!(backend.open_calls.get(), 1);
        assert_eq!(backend.remove_non_directory_calls.get(), 0);
        assert_eq!(backend.remove_empty_directory_calls.get(), 1);
        assert_eq!(backend.before_remove_calls.get(), 1);
    }

    #[test]
    fn test_non_directory_entry_is_removed_without_opening_directory() {
        let mut backend = RecordingBackend::new();
        backend.child_is_directory = false;
        backend.yield_child.set(true);
        remove_directory_tree(
            &backend,
            &PathBuf::from("tree"),
            LocalDeleteOptions::new().with_recursive(),
            Instant::now(),
        )
        .expect("non-directory root entry should be removed");

        assert_eq!(backend.metadata_calls.get(), 2);
        assert_eq!(backend.open_calls.get(), 1);
        assert_eq!(backend.remove_non_directory_calls.get(), 1);
        assert_eq!(backend.remove_empty_directory_calls.get(), 1);
        assert_eq!(backend.before_remove_calls.get(), 2);
    }

    #[test]
    fn test_backend_failures_preserve_effect_state_contracts() {
        let mut open_backend = RecordingBackend::new();
        open_backend.open_error = Some(io::ErrorKind::PermissionDenied);
        let open_error = remove_directory_tree(
            &open_backend,
            &PathBuf::from("tree"),
            LocalDeleteOptions::new().with_recursive(),
            Instant::now(),
        )
        .expect_err("directory open failure should be returned");
        assert_eq!(open_error.io_error_kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            open_error.effect_state(),
            Some(crate::error::LocalFileEffectState::Unchanged)
        );
        assert_eq!(open_backend.before_remove_calls.get(), 0);
        assert_eq!(open_backend.remove_non_directory_calls.get(), 0);
        assert_eq!(open_backend.remove_empty_directory_calls.get(), 0);

        let mut remove_backend = RecordingBackend::new();
        remove_backend.child_is_directory = false;
        remove_backend.yield_child.set(true);
        remove_backend.remove_non_directory_error = Some(io::ErrorKind::PermissionDenied);
        let remove_error = remove_directory_tree(
            &remove_backend,
            &PathBuf::from("tree"),
            LocalDeleteOptions::new().with_recursive(),
            Instant::now(),
        )
        .expect_err("non-directory removal failure should be returned");
        assert_eq!(remove_error.io_error_kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            remove_error.effect_state(),
            Some(crate::error::LocalFileEffectState::Indeterminate)
        );
        assert_eq!(remove_backend.remove_non_directory_calls.get(), 1);
        assert_eq!(remove_backend.remove_empty_directory_calls.get(), 0);
        assert_eq!(remove_backend.before_remove_calls.get(), 1);

        let mut non_directory_root_backend = RecordingBackend::new();
        non_directory_root_backend.root_is_directory = false;
        let root_error = remove_directory_tree(
            &non_directory_root_backend,
            &PathBuf::from("tree"),
            LocalDeleteOptions::new().with_recursive(),
            Instant::now(),
        )
        .expect_err("non-directory root should be rejected");
        assert_eq!(root_error.io_error_kind(), io::ErrorKind::NotADirectory);
        assert_eq!(
            root_error.effect_state(),
            Some(crate::error::LocalFileEffectState::Unchanged)
        );
        assert_eq!(non_directory_root_backend.metadata_calls.get(), 1);
        assert_eq!(non_directory_root_backend.open_calls.get(), 0);
        assert_eq!(non_directory_root_backend.remove_non_directory_calls.get(), 0);
        assert_eq!(non_directory_root_backend.remove_empty_directory_calls.get(), 0);
        assert_eq!(non_directory_root_backend.before_remove_calls.get(), 0);
    }
}
