// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Per-operation resource limits. None inherits the executor's limit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalOperationOptions {
    /// Maximum bytes retained for this operation, if overridden.
    pub max_owned_bytes: Option<usize>,
    /// Maximum native handles retained for this operation, if overridden.
    pub max_open_handles: Option<usize>,
    /// Maximum temporary disk bytes retained, if overridden.
    pub max_disk_bytes: Option<u64>,
}
