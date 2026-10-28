// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsString;
use crate::directory::LocalResolutionOrigin;

use super::symlink_identity::SymlinkIdentity;

/// One pending step in handle-relative symbolic-link expansion.
#[derive(Clone, Debug)]
pub(super) enum ResolutionStep {
    /// Restarts resolution at the virtual root.
    ResetRoot,
    /// Treats a current-directory step as a no-op after budget accounting.
    Current(LocalResolutionOrigin),
    /// Resolves the real current directory before removing a component.
    Parent(LocalResolutionOrigin),
    /// Appends one normal namespace component.
    Normal(
        /// Native component to append to the pending resolution queue.
        OsString,
        /// Input or symlink-target spelling that introduced this component.
        LocalResolutionOrigin,
    ),
    /// Ends expansion of the identified symbolic link.
    EndSymlink(
        /// Identity removed from the active-link set after expansion.
        SymlinkIdentity,
    ),
}
