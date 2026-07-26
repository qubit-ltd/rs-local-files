// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Copy type-conflict policy.

/// Conflict policy when source and destination entry types differ.
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::copy::TypeConflictPolicy;
///
/// TypeConflictPolicy::default();
/// ```
///
/// ```compile_fail
/// use qubit_local_files::copy::TypeConflictPolicy;
///
/// fn classify(policy: TypeConflictPolicy) {
///     match policy {
///         TypeConflictPolicy::Fail => {}
///         TypeConflictPolicy::Replace => {}
///     }
/// }
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalCopyTypeConflictPolicy {
    /// Fail without removing the destination entry.
    #[default]
    Fail,
    /// Remove the destination entry, including a directory tree, and replace
    /// it with the source entry.
    Replace,
}
