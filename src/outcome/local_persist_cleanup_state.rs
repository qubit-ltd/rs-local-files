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
/// use qubit_io::Output;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::outcome::LocalPersistCleanupState;
///
/// let filesystem = LocalFileSystem::host()?;
/// let mut temporary = filesystem.create_temp_file()?;
/// temporary.write_fully(b"payload")?;
/// let target_directory = tempfile::tempdir_in(std::env::current_dir()?)?;
/// let target = target_directory.path().join("persisted");
/// let persistence = temporary.persist(&target);
/// #[cfg(target_os = "freebsd")]
/// {
///     let failure = persistence.expect_err("FreeBSD temporary persistence is unsupported");
///     assert_eq!(failure.kind(), qubit_local_files::error::LocalFileErrorKind::Unsupported);
/// }
/// #[cfg(not(target_os = "freebsd"))]
/// {
///     let outcome = persistence?;
///     assert_eq!(outcome.cleanup_state(), LocalPersistCleanupState::Complete);
/// }
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
