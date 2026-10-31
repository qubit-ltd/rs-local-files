// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Whether a security observation must provide the selected native facts.
///
/// `Optional` still attempts the query and returns `None` for unsupported
/// facts. `Required` turns unavailable or unsupported selected facts into
/// `RequirementNotMet`; permission, I/O, malformed-data, and identity-change
/// errors remain distinct failures.
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
    /// Fail unless identity, length, and native change times are available.
    Required,
}
