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

#[cfg(test)]
pub(crate) fn reset_test_observation_count() {
    TREE_ENTRY_OBSERVATIONS.with(|count| count.set(0));
}

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
    directory: LocalDirectoryHandle,
    walker: LocalDirectoryWalker,
}

impl LocalTreeObserver {
    pub(crate) fn new(directory: LocalDirectoryHandle, walker: LocalDirectoryWalker) -> Self {
        Self { directory, walker }
    }
}

impl Iterator for LocalTreeObserver {
    type Item = LocalResult<LocalTreeEntryObservation>;

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
