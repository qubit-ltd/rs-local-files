// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by structured error integration tests.

/// Stable classification of local filesystem failures.
///
/// # Examples
///
/// ```
/// use qubit_local_files::error::LocalFileErrorKind;
///
/// assert_eq!(LocalFileErrorKind::NotFound, LocalFileErrorKind::NotFound);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[must_use]
pub enum LocalFileErrorKind {
    /// A native path or filename violates the operation's path contract.
    InvalidPath,
    /// Operation options are internally inconsistent or out of range.
    InvalidOptions,
    /// The resource is not in a state that permits the operation.
    InvalidState,
    /// The requested entry does not exist.
    NotFound,
    /// The requested destination already exists.
    AlreadyExists,
    /// A path component expected to be a directory is not a directory.
    NotDirectory,
    /// An operation expected a file but found a directory.
    IsDirectory,
    /// Source and destination entry kinds conflict.
    TypeConflict,
    /// The operating system denied the operation.
    PermissionDenied,
    /// The platform cannot provide the requested operation.
    Unsupported,
    /// A required semantic guarantee cannot be provided.
    RequirementNotMet,
    /// A configured or operating-system resource limit was reached.
    ResourceLimit,
    /// Native metadata or stored data failed structural validation.
    DataCorruption,
    /// Publication did not complete.
    PublicationIncomplete,
    /// The final namespace state cannot be determined safely.
    Indeterminate,
    /// An ordinary native I/O operation failed.
    Io,
}
