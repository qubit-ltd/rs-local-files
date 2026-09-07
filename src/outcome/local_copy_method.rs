// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.

/// Native method used to complete a copy operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
#[non_exhaustive]
pub enum LocalCopyMethod {
    /// One non-directory entry was handled.
    ///
    /// Regular-file bytes use same-directory staging. Symbolic links are
    /// created as link entries without staging file contents. This method is
    /// also reported when conflict policy skips the entry; inspect statistics
    /// and achieved guarantees to distinguish these cases.
    StagedFile,
    /// A directory tree was traversed and each file was staged independently.
    Recursive,
}
