// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;

use crate::observation::LocalFileObservation;

/// Observation of one entry yielded from a staged tree.
///
/// The path is relative to the stage root. Enumeration and metadata
/// observation are separate operations, so the entry can change or disappear
/// between them; a collection of these values is not an atomic tree snapshot.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::{LocalDirectoryOpenOptions, LocalListOptions, LocalTempDirectoryOptions};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let scratch = tempfile::tempdir()?;
/// let parent = LocalFileSystem::host()?.open_directory_with_options(
///     scratch.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let mut stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// stage.write_file_new(Path::new("payload"), &mut &b"data"[..], 4)?;
/// let entries = stage.observe_tree_with_options(&LocalListOptions::new())?;
/// let entry = entries.into_iter().collect::<Result<Vec<_>, _>>()?
///     .into_iter().find(|entry| entry.relative_path() == Path::new("payload"))
///     .expect("payload was observed");
/// assert_eq!(entry.relative_path(), Path::new("payload"));
/// let _captured_metadata = entry.observation();
/// stage.cleanup()?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct LocalTreeEntryObservation {
    /// Entry path relative to the root of the staged directory.
    relative_path: PathBuf,
    /// Native metadata observation captured after enumeration.
    observation: LocalFileObservation,
}

impl LocalTreeEntryObservation {
    /// Creates an observation for a stage-relative entry.
    pub(crate) const fn new(relative_path: PathBuf, observation: LocalFileObservation) -> Self {
        Self {
            relative_path,
            observation,
        }
    }

    /// Returns this entry's path relative to the staged directory.
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns the native observation captured for this entry.
    #[must_use]
    pub const fn observation(&self) -> &LocalFileObservation {
        &self.observation
    }
}
