// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Whether an observation must provide the complete facts requested by its
/// entry point.
///
/// Ordinary native observation uses this to control its identity and timestamp
/// extension; normalized metadata remains required independently. Security
/// observation uses it for each selected filesystem, access, or link-count
/// fact. `Optional` still attempts native queries and omits facts only under
/// that entry point's documented unsupported or query-failure rules. `Required`
/// reports incomplete requested facts as `RequirementNotMet`. Error kinds and
/// retained native error sources can differ by entry point and platform.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::{LocalObservationRequirement, LocalSecurityObservationOptions};
/// let options = LocalSecurityObservationOptions::new()
///     .with_filesystem(LocalObservationRequirement::Optional)
///     .with_access(LocalObservationRequirement::Required);
/// let _ = options;
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalObservationRequirement {
    /// Return available native details when possible.
    #[default]
    Optional,
    /// Fail unless the complete facts requested by the current entry point are
    /// available.
    Required,
}
