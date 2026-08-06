// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Side-effect-free destination action selected for a copy entry.
// qubit-style: allow source-test-pair

/// Action selected after comparing one source and destination entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyDestinationAction {
    /// Create a destination that is currently absent.
    Create,
    /// Traverse into an existing destination directory.
    Merge,
    /// Replace an existing destination entry.
    Replace,
    /// Preserve the destination and ignore the source entry.
    Skip,
}
