// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Publication fact for a staged directory tree.
///
/// Inspect this value alongside the operation's result to determine whether
/// the destination name was installed or whether the final state is unknown.
///
/// # Examples
///
/// ```
/// use qubit_local_files::outcome::LocalTreePublication;
///
/// let publication = LocalTreePublication::Published;
/// assert_eq!(publication, LocalTreePublication::Published);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTreePublication {
    /// The stage was not published.
    NotPublished,
    /// The stage was published under its destination name.
    Published,
    /// The final namespace state could not be determined.
    Indeterminate,
}
