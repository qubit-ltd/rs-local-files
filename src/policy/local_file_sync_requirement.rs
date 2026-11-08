// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Controls whether an operation explicitly synchronizes a file's contents
/// and metadata before publishing it.
///
/// `Required` makes synchronization failure prevent publication. `Preferred`
/// attempts synchronization but permits the operation to continue if it
/// fails. `NotRequired` does not request explicit file synchronization.
///
/// # Examples
///
/// ```
/// use qubit_local_files::policy::LocalFileSyncRequirement;
///
/// let requirement = LocalFileSyncRequirement::Required;
/// assert_ne!(requirement, LocalFileSyncRequirement::NotRequired);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[must_use]
#[non_exhaustive]
pub enum LocalFileSyncRequirement {
    /// Do not request an explicit file synchronization.
    #[default]
    NotRequired,
    /// Attempt synchronization and report whether it succeeded.
    Preferred,
    /// Require synchronization before publishing the file.
    Required,
}
