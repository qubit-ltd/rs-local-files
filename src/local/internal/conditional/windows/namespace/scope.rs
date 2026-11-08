// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Access strategies and location context for a Windows conditional namespace.

use std::path::PathBuf;

/// Describes whether namespace operations use host paths or a retained root.
#[derive(Debug)]
pub(super) enum Scope {
    /// Accesses entries beneath a host filesystem parent directory.
    Host {
        /// Parent location used for host filesystem operations.
        parent_path: PathBuf,
    },
    /// Accesses entries relative to an already-open rooted directory.
    Rooted {
        /// Absolute namespace location used only to identify entries.
        namespace_absolute: PathBuf,
    },
}
