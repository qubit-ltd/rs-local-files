// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Directory-handle budget policies.

/// Controls how a recursive walker continues after reaching its open-handle
/// budget.
///
/// `Fail` stops the walk with a resource-limit error. `Reopen` releases active
/// directory readers and reopens them as the walk resumes, reducing the number
/// of simultaneously open handles at the cost of additional filesystem work.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalDirectoryReopenPolicy;
/// use qubit_local_files::options::LocalListOptions;
///
/// let options = LocalListOptions::new()
///     .with_max_open_directories(1)
///     .with_reopen_policy(LocalDirectoryReopenPolicy::Fail);
/// assert_eq!(options.reopen_policy(), LocalDirectoryReopenPolicy::Fail);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalDirectoryReopenPolicy {
    /// Return [`crate::LocalFileErrorKind::ResourceLimit`].
    Fail,
    /// Close active readers and reopen them while resuming the walk.
    Reopen,
}
