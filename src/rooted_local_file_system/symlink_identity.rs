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
    Native(u64, u64),
    /// Namespace path used when native identity is unavailable.
    NamespacePath(PathBuf),
}
