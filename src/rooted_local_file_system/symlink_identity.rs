// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Identity retained only while one symbolic-link target is expanding.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum SymlinkIdentity {
    /// Stable native device and file identity.
    Native(
        /// Native device or volume identifier.
        u64,
        /// Native file identifier within the device or volume.
        u64,
    ),
    /// Namespace path used when native identity is unavailable.
    NamespacePath(
        /// Authority-relative path serving as the fallback identity.
        PathBuf,
    ),
}
