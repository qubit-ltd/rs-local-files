// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by public capability integration tests.

/// Unit used by a native path-length limit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalPathLengthUnit {
    /// Native path bytes, as used by Unix APIs.
    Bytes,
    /// UTF-16 code units, as used by Windows wide-character APIs.
    Utf16CodeUnits,
}
