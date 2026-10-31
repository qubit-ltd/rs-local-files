// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Identifies whether resolution is opening the authority root or a descendant.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalResolutionScope;
///
/// fn is_root(scope: LocalResolutionScope) -> bool {
///     matches!(scope, LocalResolutionScope::OpeningRoot)
/// }
/// assert!(is_root(LocalResolutionScope::OpeningRoot));
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalResolutionScope {
    /// The native path is being resolved to establish the root authority.
    OpeningRoot,
    /// The path is being resolved beneath an established authority.
    Descendant,
}
