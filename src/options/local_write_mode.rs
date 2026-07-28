// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

/// Publication mode for a local file writer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalWriteMode {
    /// Publish only when the destination does not exist.
    CreateNew,
    /// Publish a new regular-file entry or replace an existing destination
    /// entry.
    CreateOrReplace,
    /// Directly append bytes to an existing regular file.
    Append,
}
