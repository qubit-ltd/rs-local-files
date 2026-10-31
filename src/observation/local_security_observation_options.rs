// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::observation::LocalObservationRequirement;
/// Selects security facts and bounds ACL data retained by an observation.
///
/// Each query defaults to `Optional`; ACL limits default to `None` (no caller
/// limit), while a configured zero is a real zero-entry or zero-byte limit.
/// Optional queries omit a fact only when the mechanism explicitly reports it
/// as unsupported. Required queries fail with `RequirementNotMet` when a
/// requested fact is unsupported or incomplete. Other query errors, including
/// permission, ordinary I/O, malformed ACL data, and detected identity changes,
/// remain errors. Unix preserves native I/O error kinds; Windows converts
/// unsupported required facts to `RequirementNotMet` and preserves other
/// errors.
///
/// Optional requirements still attempt their native query. Only explicitly
/// unsupported facts become `None`; access denial, ordinary I/O failures,
/// malformed ACLs, and changes detected during the observation return errors.
/// On Unix, filesystem facts are implemented on Linux and
/// macOS, while complete ACL access facts are currently queried on Linux. On
/// Windows, the native filesystem and security descriptor queries are used.
///
/// ```
/// use qubit_local_files::observation::{LocalObservationRequirement, LocalSecurityObservationOptions};
/// let options = LocalSecurityObservationOptions::new()
///     .with_filesystem(LocalObservationRequirement::Optional)
///     .with_access(LocalObservationRequirement::Required)
///     .with_max_acl_entries(128)
///     .with_max_acl_bytes(16 * 1024);
/// # let _ = options;
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct LocalSecurityObservationOptions {
    /// Requirement for native filesystem facts.
    filesystem: LocalObservationRequirement,
    /// Requirement for native access-control facts.
    access: LocalObservationRequirement,
    /// Requirement for native link-count facts.
    link_count: LocalObservationRequirement,
    /// Maximum ACL entries decoded; `None` leaves the count unrestricted.
    max_acl_entries: Option<usize>,
    /// Maximum ACL bytes read and retained; `None` leaves the byte budget
    /// unrestricted.
    max_acl_bytes: Option<usize>,
}
impl LocalSecurityObservationOptions {
    /// Creates options with all queries optional and no additional ACL limits.
    ///
    /// Optional means the query is still attempted. Explicitly unsupported
    /// facts can be absent; permission, ordinary I/O, malformed-data, and
    /// identity-change failures remain errors.
    pub const fn new() -> Self {
        Self {
            filesystem: LocalObservationRequirement::Optional,
            access: LocalObservationRequirement::Optional,
            link_count: LocalObservationRequirement::Optional,
            max_acl_entries: None,
            max_acl_bytes: None,
        }
    }
    /// Sets whether filesystem details are optional or required.
    ///
    /// `Optional` retains available facts and represents unavailable facts as
    /// `None`; `Required` returns an error if the selected facts are missing.
    pub const fn with_filesystem(mut self, v: LocalObservationRequirement) -> Self {
        self.filesystem = v;
        self
    }
    /// Sets whether access-control details are optional or required.
    ///
    /// `Optional` still performs the native query. `Required` fails when the
    /// requested ACL or access facts are unavailable.
    pub const fn with_access(mut self, v: LocalObservationRequirement) -> Self {
        self.access = v;
        self
    }
    /// Sets whether the native link count is optional or required.
    ///
    /// `Optional` permits an unavailable count; `Required` makes its absence
    /// an unmet requirement.
    pub const fn with_link_count(mut self, v: LocalObservationRequirement) -> Self {
        self.link_count = v;
        self
    }
    /// Caps the number of ACL entries decoded; zero rejects any entry.
    pub const fn with_max_acl_entries(mut self, v: usize) -> Self {
        self.max_acl_entries = Some(v);
        self
    }
    /// Caps ACL bytes read and retained; zero permits no ACL bytes.
    pub const fn with_max_acl_bytes(mut self, v: usize) -> Self {
        self.max_acl_bytes = Some(v);
        self
    }
    /// Returns the selected filesystem fact requirement for native backends.
    pub(crate) const fn filesystem(&self) -> LocalObservationRequirement {
        self.filesystem
    }
    /// Returns the selected access fact requirement for native backends.
    pub(crate) const fn access(&self) -> LocalObservationRequirement {
        self.access
    }
    #[allow(dead_code)] // Read by target-gated Unix and Windows security backends.
    /// Returns the selected link-count requirement for native backends.
    pub(crate) const fn link_count(&self) -> LocalObservationRequirement {
        self.link_count
    }
    #[cfg(any(target_os = "linux", windows))]
    /// Returns the ACL entry ceiling; `None` means no caller-specified ceiling.
    pub(crate) const fn max_acl_entries(&self) -> Option<usize> {
        self.max_acl_entries
    }
    #[cfg(any(target_os = "linux", windows))]
    /// Returns the ACL byte ceiling; `None` means no caller-specified ceiling.
    pub(crate) const fn max_acl_bytes(&self) -> Option<usize> {
        self.max_acl_bytes
    }
}
