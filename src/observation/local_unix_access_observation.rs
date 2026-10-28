// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalAclPresence;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Unix mode, owner, and ACL presence facts observed from an object.
///
/// These facts are observational and may become stale immediately; they do not
/// guarantee that a later operation will be permitted.
pub struct LocalUnixAccessObservation {
    mode: u32,
    owner_is_current_user: bool,
    access_acl: LocalAclPresence,
    default_acl: LocalAclPresence,
}
impl LocalUnixAccessObservation {
    #[cfg(unix)]
    #[allow(dead_code)] // Consumed when T03 connects Unix security queries.
    pub(crate) const fn new(
        mode: u32,
        owner_is_current_user: bool,
        access_acl: LocalAclPresence,
        default_acl: LocalAclPresence,
    ) -> Self {
        Self {
            mode,
            owner_is_current_user,
            access_acl,
            default_acl,
        }
    }
    /// Returns permission and special mode bits (`st_mode & 0o7777`).
    pub const fn mode(&self) -> u32 {
        self.mode
    }
    /// Returns whether the owner matches the current effective user.
    pub const fn owner_is_current_user(&self) -> bool {
        self.owner_is_current_user
    }
    /// Returns the access ACL status.
    pub const fn access_acl(&self) -> LocalAclPresence {
        self.access_acl
    }
    /// Returns the default ACL status.
    pub const fn default_acl(&self) -> LocalAclPresence {
        self.default_acl
    }
}
