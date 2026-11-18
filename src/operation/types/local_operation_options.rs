// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Per-operation upper bounds for resources retained by one operation.
///
/// `None` inherits the corresponding executor-wide limit. `Some` may tighten,
/// but cannot exceed, that executor limit; zero or an invalid limit causes
/// submission rejection. These limits do not reserve capacity by themselves:
/// accepted operations also compete for the shared executor pools.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalOperationOptions {
    /// Maximum owned payload bytes this operation may retain, if overridden.
    pub max_owned_bytes: Option<usize>,
    /// Maximum native handles this operation may retain, if overridden.
    pub max_open_handles: Option<usize>,
    /// Maximum temporary disk bytes this operation may reserve, if overridden.
    pub max_disk_bytes: Option<u64>,
}
