// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by structured error integration tests.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::LocalFileErrorKind;
use super::LocalFileErrorSource;
use super::LocalFileOperation;
use super::LocalPathCodecError;

/// Structured failure from a local filesystem operation.
#[derive(Debug)]
pub struct LocalFileError {
    /// Stable failure classification.
    kind: LocalFileErrorKind,
    /// Operation that failed.
    operation: LocalFileOperation,
    /// Primary native path involved in the operation.
    path: Option<PathBuf>,
    /// Secondary or destination native path.
    target: Option<PathBuf>,
    /// Stable explanation for policy or validation failures.
    reason: Option<&'static str>,
    /// Typed source retained from the originating failure.
    source: Option<LocalFileErrorSource>,
}

impl LocalFileError {
    /// Creates a structured error without native I/O context.
    ///
    /// # Parameters
    ///
    /// - `kind`: Stable failure classification.
    /// - `operation`: Operation that failed.
    #[must_use]
    #[inline(always)]
    pub const fn new(kind: LocalFileErrorKind, operation: LocalFileOperation) -> Self {
        Self {
            kind,
            operation,
            path: None,
            target: None,
            reason: None,
            source: None,
        }
    }

    /// Converts a native I/O failure and preserves path context.
    ///
    /// # Parameters
    ///
    /// - `operation`: Operation that failed.
    /// - `path`: Optional primary path.
    /// - `target`: Optional destination path.
    /// - `source`: Native I/O error.
    ///
    /// # Returns
    ///
    /// A structured local filesystem error.
    #[must_use]
    #[inline(always)]
    pub fn from_io(
        operation: LocalFileOperation,
        path: Option<PathBuf>,
        target: Option<PathBuf>,
        source: io::Error,
    ) -> Self {
        Self {
            kind: classify_io_error(&source),
            operation,
            path,
            target,
            reason: None,
            source: Some(LocalFileErrorSource::Io(source)),
        }
    }

    /// Converts a canonical path codec failure and preserves path context.
    ///
    /// # Parameters
    ///
    /// - `operation`: Operation that failed while converting a path.
    /// - `path`: Optional primary native path context.
    /// - `error`: Canonical path codec failure to retain as the typed source.
    ///
    /// # Returns
    ///
    /// A structured invalid-path error whose source is `PathCodec(error)`.
    #[must_use]
    #[allow(dead_code)]
    #[inline]
    pub(crate) fn from_path_codec(
        operation: LocalFileOperation,
        path: Option<PathBuf>,
        error: LocalPathCodecError,
    ) -> Self {
        Self {
            kind: LocalFileErrorKind::InvalidPath,
            operation,
            path,
            target: None,
            reason: None,
            source: Some(LocalFileErrorSource::PathCodec(error)),
        }
    }

    /// Adds a primary path to this error.
    ///
    /// # Parameters
    ///
    /// - `path`: Native path that was being accessed.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[must_use]
    #[inline(always)]
    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    /// Adds a destination path to this error.
    ///
    /// # Parameters
    ///
    /// - `target`: Native destination path.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[must_use]
    #[inline(always)]
    pub fn with_target(mut self, target: PathBuf) -> Self {
        self.target = Some(target);
        self
    }

    /// Adds a stable explanation for the failure.
    ///
    /// # Parameters
    ///
    /// - `reason`: Static human-readable explanation of the failed requirement
    ///   or validation rule.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[must_use]
    #[inline(always)]
    pub const fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }

    /// Returns the stable failure classification.
    #[inline(always)]
    pub const fn kind(&self) -> LocalFileErrorKind {
        self.kind
    }

    /// Returns the operation that failed.
    #[inline(always)]
    pub const fn operation(&self) -> LocalFileOperation {
        self.operation
    }

    /// Returns the primary path, or `None` when no path applies.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Returns the destination path, or `None` for single-path operations.
    #[must_use]
    #[inline]
    pub fn target(&self) -> Option<&Path> {
        self.target.as_deref()
    }

    /// Returns the stable human-readable explanation, when one was provided.
    #[must_use]
    #[inline(always)]
    pub const fn reason(&self) -> Option<&str> {
        self.reason
    }

    /// Returns the typed source retained from the originating failure.
    ///
    /// # Returns
    ///
    /// `Some` contains an I/O or path codec source; `None` means this error
    /// was constructed without an originating source.
    #[must_use]
    #[inline(always)]
    pub const fn typed_source(&self) -> Option<&LocalFileErrorSource> {
        self.source.as_ref()
    }

    /// Returns the retained native I/O source, when the failure originated in
    /// a standard-library I/O operation.
    #[must_use]
    #[inline]
    pub fn io_error(&self) -> Option<&io::Error> {
        match self.source.as_ref() {
            Some(LocalFileErrorSource::Io(error)) => Some(error),
            _ => None,
        }
    }

    /// Returns the standard I/O kind represented by this structured error.
    #[must_use]
    #[inline]
    pub fn io_error_kind(&self) -> io::ErrorKind {
        standard_io_error_kind(self)
    }

    /// Consumes the error and returns its typed source, if present.
    ///
    /// # Returns
    ///
    /// `Some` contains an I/O or path codec source; `None` means this error
    /// was constructed without an originating source.
    #[must_use]
    #[inline(always)]
    pub fn into_source(self) -> Option<LocalFileErrorSource> {
        self.source
    }

    /// Consumes this structured error as a standard I/O error.
    ///
    /// # Returns
    ///
    /// An I/O error that preserves the originating native kind when available
    /// and retains this structured error as its source.
    #[must_use]
    pub fn into_io_error(self) -> io::Error {
        io::Error::new(self.io_error_kind(), self)
    }

    /// Reclassifies an error while retaining its native source and paths.
    ///
    /// # Parameters
    ///
    /// - `kind`: More precise classification established by the caller.
    ///
    /// # Returns
    ///
    /// The reclassified error.
    #[inline(always)]
    pub(crate) const fn with_kind(mut self, kind: LocalFileErrorKind) -> Self {
        self.kind = kind;
        self
    }
}

/// Selects the standard I/O kind to expose when adapting this error.
///
/// Native I/O sources retain their exact kind. Errors without one use the
/// closest stable local classification.
#[inline]
fn standard_io_error_kind(error: &LocalFileError) -> io::ErrorKind {
    match error.source.as_ref() {
        Some(LocalFileErrorSource::Io(source)) => source.kind(),
        Some(LocalFileErrorSource::PathCodec(_)) => io::ErrorKind::InvalidInput,
        None => match error.kind {
            LocalFileErrorKind::AlreadyExists => io::ErrorKind::AlreadyExists,
            LocalFileErrorKind::InvalidPath
            | LocalFileErrorKind::InvalidOptions
            | LocalFileErrorKind::InvalidState => io::ErrorKind::InvalidInput,
            LocalFileErrorKind::NotDirectory => io::ErrorKind::NotADirectory,
            LocalFileErrorKind::IsDirectory => io::ErrorKind::IsADirectory,
            LocalFileErrorKind::NotFound => io::ErrorKind::NotFound,
            LocalFileErrorKind::PermissionDenied => io::ErrorKind::PermissionDenied,
            LocalFileErrorKind::ResourceLimit => io::ErrorKind::StorageFull,
            LocalFileErrorKind::DataCorruption => io::ErrorKind::InvalidData,
            LocalFileErrorKind::RequirementNotMet | LocalFileErrorKind::Unsupported => {
                io::ErrorKind::Unsupported
            }
            _ => io::ErrorKind::Other,
        },
    }
}

impl fmt::Display for LocalFileError {
    /// Formats the structured operation and available native path context.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} failed with {:?}",
            self.operation, self.kind
        )?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        if let Some(path) = &self.path {
            write!(formatter, " at {}", path.display())?;
        }
        if let Some(target) = &self.target {
            write!(formatter, " targeting {}", target.display())?;
        }
        if let Some(source) = &self.source {
            write!(formatter, "; caused by {source}")?;
        }
        Ok(())
    }
}

impl Error for LocalFileError {
    /// Returns the concrete I/O or path codec source, if present.
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().and_then(Error::source)
    }
}

/// Classifies an operating-system I/O error without discarding its source.
///
/// # Parameters
///
/// - `error`: Native I/O failure to classify.
///
/// # Returns
///
/// The stable local error kind corresponding to the native error.
#[inline]
fn classify_io_error(error: &io::Error) -> LocalFileErrorKind {
    match error.kind() {
        io::ErrorKind::NotFound => LocalFileErrorKind::NotFound,
        io::ErrorKind::AlreadyExists => LocalFileErrorKind::AlreadyExists,
        io::ErrorKind::NotADirectory => LocalFileErrorKind::NotDirectory,
        io::ErrorKind::IsADirectory => LocalFileErrorKind::IsDirectory,
        io::ErrorKind::PermissionDenied => LocalFileErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput => LocalFileErrorKind::InvalidPath,
        io::ErrorKind::InvalidData => LocalFileErrorKind::DataCorruption,
        io::ErrorKind::Unsupported => LocalFileErrorKind::Unsupported,
        io::ErrorKind::OutOfMemory | io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => {
            LocalFileErrorKind::ResourceLimit
        }
        _ => LocalFileErrorKind::Io,
    }
}
