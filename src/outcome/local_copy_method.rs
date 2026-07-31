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
    /// Regular file bytes were copied into same-directory staging and
    /// published.
    StagedFile,
    /// A directory tree was traversed and each file was staged independently.
    Recursive,
}
