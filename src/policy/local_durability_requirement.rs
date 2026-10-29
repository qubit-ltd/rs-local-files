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
/// `Required` makes the operation fail if any synchronization point required
/// by its selected protocol fails. Writers and file copies include file data
/// and relevant namespace updates; rename covers namespace updates only.
/// Tree file synchronization is selected separately from publication namespace
/// durability. `Preferred` attempts synchronization but allows the operation
/// to succeed if native synchronization fails. `NotRequired` does not request
/// explicit synchronization. The guarantee follows the platform's
/// synchronization semantics and does not promise that every operation
/// synchronizes file data.
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
    /// All synchronization points required by the selected operation protocol
    /// must complete.
    Required,
    /// Prefer synchronization but permit an explicitly reported downgrade.
    Preferred,
    /// Do not require explicit synchronization.
    #[default]
    NotRequired,
}
