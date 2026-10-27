// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Write access state after a possibly partial temporary-file sealing attempt.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTempAccessState {
    /// The retained owner still permits writes.
    Writable,
    /// The retained owner is restricted to read access.
    ReadOnly,
    /// The native access state could not be established.
    Indeterminate,
}
