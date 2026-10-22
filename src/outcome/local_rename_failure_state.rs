// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Outcome states are covered through public rename integration tests.
//! States proven when a unified rename operation fails.

/// Namespace state proven by a failed unified rename operation.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::outcome::LocalRenameFailureState;
///
/// let filesystem = LocalFileSystem::host()?;
/// let source = std::env::temp_dir().join("qubit-local-files-missing-rename-source");
/// let target = std::env::temp_dir().join("qubit-local-files-rename-target");
/// let failure = filesystem
///     .rename(Path::new(&source), Path::new(&target))
///     .expect_err("source is missing");
/// assert_eq!(failure.state(), LocalRenameFailureState::Unchanged);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalRenameFailureState {
    /// The native namespace is proven unchanged.
    Unchanged,
    /// The source was renamed to the destination before a later failure.
    Renamed,
    /// Native I/O failed without proving whether the rename took effect.
    Indeterminate,
}
