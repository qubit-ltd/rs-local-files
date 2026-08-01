// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Copy conflict policy.
// qubit-style: allow source-test-pair

/// Conflict policy for existing destination file entries.
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::copy::ConflictPolicy;
///
/// ConflictPolicy::default();
/// ```
///
/// ```compile_fail
/// use qubit_local_files::copy::ConflictPolicy;
///
/// fn classify(policy: ConflictPolicy) {
///     match policy {
///         ConflictPolicy::Fail => {}
///         ConflictPolicy::Overwrite => {}
///         ConflictPolicy::Skip => {}
///     }
/// }
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalCopyConflictPolicy {
    /// Fail when a destination file entry already exists.
    #[default]
    Fail,
    /// Replace an existing destination file entry.
    Overwrite,
    /// Keep an existing destination file entry and continue copying.
    Skip,
}
