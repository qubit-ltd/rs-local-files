// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup state reported by temporary-resource persistence.

/// Cleanup state achieved after a temporary resource was published.
///
/// # Examples
///
/// ```
/// use std::io::Write;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::outcome::LocalPersistCleanupState;
///
/// let filesystem = LocalFileSystem::host()?;
/// let mut temporary = filesystem.create_temp_file()?;
/// temporary.write_all(b"payload")?;
/// let target = std::env::temp_dir().join(format!(
///     "qubit-persist-example-{}",
///     std::process::id()
/// ));
/// let outcome = temporary
///     .persist(&target)
///     .expect("temporary file must persist");
/// assert_eq!(outcome.cleanup_state(), LocalPersistCleanupState::Complete);
/// std::fs::remove_file(target)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPersistCleanupState {
    /// The private temporary sandbox was removed successfully.
    Complete,
    /// Publication succeeded, but the private temporary sandbox remains.
    ResidualSandbox,
}
