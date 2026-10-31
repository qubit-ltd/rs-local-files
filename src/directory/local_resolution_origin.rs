// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Identifies whether a path component came from caller input or a link target.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalResolutionOrigin;
///
/// fn is_link_target(origin: LocalResolutionOrigin) -> bool {
///     matches!(origin, LocalResolutionOrigin::SymlinkTarget)
/// }
/// assert!(is_link_target(LocalResolutionOrigin::SymlinkTarget));
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalResolutionOrigin {
    /// The component came from the operation's input path.
    Input,
    /// The component came from the native target of a followed symlink.
    SymlinkTarget,
}
