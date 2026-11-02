// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured successful temporary-resource persistence outcomes.

use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalPersistCleanupState;
use crate::LocalPersistMethod;

/// Guarantees actually achieved while persisting a temporary resource.
///
/// # Examples
///
/// Inspect publication and cleanup independently. A cleanup error does not
/// undo a successful publication; handle a residual sandbox through the
/// application's recovery flow rather than publishing again or deleting by
/// path alone.
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalTempFileOptions;
/// use qubit_local_files::outcome::LocalPersistCleanupState;
///
/// let fixture = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let temporary = filesystem.create_temp_file_with_options(
///     &LocalTempFileOptions::new().with_parent(fixture.path()),
/// )?;
/// let outcome = temporary.keep()?;
/// assert!(outcome.path().is_absolute());
/// let _achieved_guarantees = (outcome.atomic(), outcome.durable());
/// if outcome.cleanup_state() == LocalPersistCleanupState::Complete {
///     assert!(outcome.cleanup_error().is_none());
///     std::fs::remove_file(outcome.path())?;
/// } else {
///     // Publication succeeded. Return these facts to the application's recovery
///     // flow instead of publishing again or deleting a sandbox by path alone.
///     assert!(outcome.cleanup_error().is_some());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
#[derive(Debug)]
pub struct LocalPersistOutcome {
    /// Namespace-absolute path at which the resource was published.
    path: PathBuf,
    /// Native publication method.
    method: LocalPersistMethod,
    /// Whether publication was atomic.
    atomic: bool,
    /// Whether persistence durability was synchronized.
    durable: bool,
    /// Cleanup error retained after successful publication, when any.
    cleanup_error: Option<LocalFileError>,
}

impl LocalPersistOutcome {
    /// Creates a temporary-resource persistence outcome.
    pub(crate) const fn new(
        path: PathBuf,
        method: LocalPersistMethod,
        atomic: bool,
        durable: bool,
        cleanup_error: Option<LocalFileError>,
    ) -> Self {
        Self {
            path,
            method,
            atomic,
            durable,
            cleanup_error,
        }
    }

    /// Returns the namespace-absolute published path.
    #[must_use]
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the native publication method.
    #[must_use = "the persistence method should be inspected"]
    #[inline]
    pub const fn method(&self) -> LocalPersistMethod {
        self.method
    }

    /// Reports whether publication was atomic.
    #[must_use]
    #[inline]
    pub const fn atomic(&self) -> bool {
        self.atomic
    }

    /// Reports whether temporary-file contents and destination namespace were
    /// both synchronized.
    ///
    /// Temporary-directory persistence always reports `false` because the
    /// guard cannot prove synchronization of arbitrary descendant contents.
    #[must_use]
    #[inline]
    pub const fn durable(&self) -> bool {
        self.durable
    }

    /// Returns the cleanup state achieved after publication.
    #[must_use = "the sandbox cleanup state should be inspected"]
    #[inline]
    pub const fn cleanup_state(&self) -> LocalPersistCleanupState {
        if self.cleanup_error.is_some() {
            LocalPersistCleanupState::ResidualSandbox
        } else {
            LocalPersistCleanupState::Complete
        }
    }

    /// Returns the cleanup error retained after successful publication.
    #[inline]
    pub const fn cleanup_error(&self) -> Option<&LocalFileError> {
        self.cleanup_error.as_ref()
    }

    /// Returns the published path and any retained cleanup error.
    #[must_use]
    #[inline]
    pub fn into_parts(self) -> (PathBuf, Option<LocalFileError>) {
        (self.path, self.cleanup_error)
    }
}
