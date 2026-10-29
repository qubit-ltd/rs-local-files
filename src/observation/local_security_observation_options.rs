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
/// Optional queries return no field only when the mechanism is explicitly
/// unsupported. Required queries fail when complete facts are unavailable;
/// permission errors, ordinary I/O errors, and malformed ACL data remain errors
/// for either requirement.
///
/// Optional requirements still attempt their native query. Only explicitly
/// unsupported or unavailable facts become `None`; access denial, ordinary
/// I/O failures, malformed ACLs, and changes detected during the observation
/// return errors. On Unix, filesystem facts are implemented on Linux and
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
    filesystem: LocalObservationRequirement,
    access: LocalObservationRequirement,
    link_count: LocalObservationRequirement,
    max_acl_entries: Option<usize>,
    max_acl_bytes: Option<usize>,
}
impl LocalSecurityObservationOptions {
    /// Creates options with all queries optional and no additional ACL limits.
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
    pub const fn with_filesystem(mut self, v: LocalObservationRequirement) -> Self {
        self.filesystem = v;
        self
    }
    /// Sets whether access-control details are optional or required.
    pub const fn with_access(mut self, v: LocalObservationRequirement) -> Self {
        self.access = v;
        self
    }
    /// Sets whether the native link count is optional or required.
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
    pub(crate) const fn filesystem(&self) -> LocalObservationRequirement {
        self.filesystem
    }
    pub(crate) const fn access(&self) -> LocalObservationRequirement {
        self.access
    }
    #[allow(dead_code)] // Read by target-gated Unix and Windows security backends.
    pub(crate) const fn link_count(&self) -> LocalObservationRequirement {
        self.link_count
    }
    pub(crate) const fn max_acl_entries(&self) -> Option<usize> {
        self.max_acl_entries
    }
    pub(crate) const fn max_acl_bytes(&self) -> Option<usize> {
        self.max_acl_bytes
    }
}
