// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use std::fs::File;

use crate::local::LocalAtomicWriter;
use crate::local::LocalRootAtomicWriter;

/// Native backend selected for one writer session.
#[derive(Debug)]
pub(crate) enum LocalFileWriterBackend {
    /// Same-directory staged publication.
    Staged(LocalAtomicWriter),
    /// Descriptor- or handle-relative same-directory staged publication.
    Rooted(LocalRootAtomicWriter),
    /// Direct append to an existing file.
    Append(File),
}
