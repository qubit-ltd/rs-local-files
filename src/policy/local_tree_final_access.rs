// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Requested final access policy for a staged directory tree.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTreeFinalAccess {
    /// Preserve each entry's current access controls.
    #[default]
    Preserve,
    /// Restrict access to the current user while retaining required use rights.
    CurrentUserOnly,
    /// Restrict access to the current user and make files read-only.
    ReadOnlyCurrentUser,
}
