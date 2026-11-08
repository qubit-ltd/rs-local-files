// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalDirectoryHandle;
use super::tree_validation::LocalTreeEntryObservation;
use crate::LocalDirectoryWalker;
use crate::LocalFileOperation;
use crate::LocalResult;

#[cfg(test)]
thread_local! {
    static TREE_ENTRY_OBSERVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Resets the thread-local count of metadata observations made by tests.
#[cfg(test)]
pub(crate) fn reset_test_observation_count() {
    TREE_ENTRY_OBSERVATIONS.with(|count| count.set(0));
}

/// Returns the thread-local number of metadata observations made by tests.
#[cfg(test)]
pub(crate) fn test_observation_count() -> usize {
    TREE_ENTRY_OBSERVATIONS.with(std::cell::Cell::get)
}

/// Lazy iterator over entries observed beneath a staged directory.
///
/// Each item is observed only when requested. Walker enumeration and the
/// subsequent metadata observation are separate native operations, so an
/// entry can change or disappear between them. An observation failure is
/// yielded for that item and does not itself terminate the walker; later items
/// depend on the walker's state. Enumeration failures follow the configured
/// [`LocalWalkErrorPolicy`](crate::options::LocalWalkErrorPolicy): `FailFast`
/// terminates after the error, while `Continue` may yield later items.
/// Exhausting the global entry budget or deadline terminates traversal under
/// either policy. An error therefore does not promise that later items exist,
/// and missing items cannot be treated as a complete validation. The iterator
/// is not a consistent snapshot of the tree.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::error::LocalFileErrorKind;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
/// use qubit_local_files::options::LocalListOptions;
/// use qubit_local_files::options::LocalTempDirectoryOptions;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let scratch = tempfile::tempdir()?;
///     let parent = LocalFileSystem::host()?.open_directory_with_options(
///         scratch.path(),
///         &LocalDirectoryOpenOptions::new(),
///     )?;
///     let mut stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
///     stage.write_file_new(Path::new("one"), &mut &b"1"[..], 1)?;
///     stage.write_file_new(Path::new("two"), &mut &b"2"[..], 1)?;
///
///     let mut observer = stage.observe_tree_with_options(
///         &LocalListOptions::new().with_max_entries(0),
///     )?;
///     assert!(matches!(observer.next(), Some(Err(ref error))
///         if error.kind() == LocalFileErrorKind::ResourceLimit));
///     assert!(observer.next().is_none());
///     drop(observer);
///     stage.cleanup()?;
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct LocalTreeObserver {
    /// Directory handle used to observe each path returned by the walker.
    directory: LocalDirectoryHandle,
    /// Incremental traversal state and its configured error policy.
    walker: LocalDirectoryWalker,
}

impl LocalTreeObserver {
    /// Creates an observer from a staged directory and its configured walker.
    ///
    /// The walker determines enumeration limits and error handling; metadata
    /// observation is performed lazily as callers request items.
    ///
    /// # Parameters
    ///
    /// - `directory`: retained authority used to observe each enumerated entry.
    /// - `walker`: configured traversal state that supplies entries and
    ///   controls traversal limits and error handling.
    pub(crate) fn new(directory: LocalDirectoryHandle, walker: LocalDirectoryWalker) -> Self {
        Self { directory, walker }
    }
}

impl Iterator for LocalTreeObserver {
    /// Each item pairs a relative path with its observed metadata, or reports
    /// the failure for that path.
    type Item = LocalResult<LocalTreeEntryObservation>;

    /// Observes the next enumerated entry when requested.
    ///
    /// Enumeration and metadata lookup are separate operations, so the entry
    /// may change or disappear between them. An item error does not necessarily
    /// end iteration; traversal limits and the walker's error policy control
    /// whether later entries can be yielded.
    fn next(&mut self) -> Option<Self::Item> {
        let entry = match self.walker.next()? {
            Ok(entry) => entry,
            Err(error) => return Some(Err(error)),
        };
        let relative = entry.relative_path().to_path_buf();
        #[cfg(test)]
        TREE_ENTRY_OBSERVATIONS.with(|count| count.set(count.get() + 1));
        Some(
            self.directory
                .observe_entry(&relative)
                .map(|observation| LocalTreeEntryObservation::new(relative, observation))
                .map_err(|error| error.with_operation(LocalFileOperation::Metadata)),
        )
    }
}
