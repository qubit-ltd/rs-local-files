// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Worker-level command failure mapped to a stable public value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTempWorkerCommandFailure {
    /// Worker command capacity was unavailable.
    Capacity,
    /// The managed resource no longer had a native owner.
    ResourceUnavailable,
    /// The worker command panicked before returning its result.
    Panicked,
}
