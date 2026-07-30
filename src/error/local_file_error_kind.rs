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
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[must_use]
pub enum LocalFileErrorKind {
    /// A path, filename, option, or state is invalid.
    InvalidInput,
    /// The requested entry does not exist.
    NotFound,
    /// The requested destination already exists.
    AlreadyExists,
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
    /// Publication did not complete.
    PublicationIncomplete,
    /// The final namespace state cannot be determined safely.
    Indeterminate,
    /// An ordinary native I/O operation failed.
    Io,
}
