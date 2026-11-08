// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsString;

use crate::directory::LocalResolutionOrigin;

/// A lexical path component annotated with the origin used during resolution.
///
/// Dot and parent components remain explicit so the resolver can apply its
/// traversal policy in order. A trailing separator is represented separately
/// to require a directory without charging another path component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NativeToken {
    /// A native name to resolve relative to the current directory.
    Normal(
        /// The operating-system-native component name, which may not be UTF-8.
        OsString,
        /// The request or symlink target that supplied this component.
        LocalResolutionOrigin,
    ),
    /// The explicit `.` component, retained for ordered resolution.
    Current(
        /// The request or symlink target that supplied this component.
        LocalResolutionOrigin,
    ),
    /// The explicit `..` component, retained for traversal-policy checks.
    Parent(
        /// The request or symlink target that supplied this component.
        LocalResolutionOrigin,
    ),
    /// A trailing separator requiring the resolved path to name a directory.
    RequireDirectory,
}
