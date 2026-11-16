// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Known byte interval modified by an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalModifiedRange {
    /// First modified byte, inclusive.
    pub start: u64,
    /// End of the modified interval, exclusive.
    pub end: u64,
}
