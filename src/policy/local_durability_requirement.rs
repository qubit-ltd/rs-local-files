// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered through operation policy integration tests.

/// Required storage durability for a completed operation.
///
/// `Required` makes synchronization failure fail the operation. `Preferred`
/// attempts synchronization but allows the operation to succeed if native
/// synchronization fails. `NotRequired` skips synchronization. The guarantee
/// applies only to synchronization points performed by that operation and
/// follows the platform's synchronization semantics.
///
/// # Examples
///
/// ```
/// use qubit_local_files::policy::LocalDurabilityRequirement;
///
/// let requirement = LocalDurabilityRequirement::Required;
/// assert_ne!(requirement, LocalDurabilityRequirement::NotRequired);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalDurabilityRequirement {
    /// File data and relevant parent namespace updates must be synchronized.
    Required,
    /// Prefer synchronization but permit an explicitly reported downgrade.
    Preferred,
    /// Do not require explicit synchronization.
    #[default]
    NotRequired,
}
