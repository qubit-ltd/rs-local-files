// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalWindowsAceObservation;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Windows owner, DACL state, and ordered ACE observations.
///
/// The ACE list preserves observed order and undecoded native details; it is
/// not a computed access decision or a guarantee of future access.
pub struct LocalWindowsAccessObservation {
    owner_sid: String,
    owner_is_current_user: bool,
    dacl_present: bool,
    dacl_is_null: bool,
    dacl_protected: bool,
    acl_valid: bool,
    aces: Vec<LocalWindowsAceObservation>,
}
impl LocalWindowsAccessObservation {
    #[cfg(windows)]
    #[allow(dead_code)] // Consumed when T04 connects Windows security queries.
    pub(crate) fn new(
        owner_sid: String,
        owner_is_current_user: bool,
        dacl_present: bool,
        dacl_is_null: bool,
        dacl_protected: bool,
        acl_valid: bool,
        aces: Vec<LocalWindowsAceObservation>,
    ) -> Self {
        Self {
            owner_sid,
            owner_is_current_user,
            dacl_present,
            dacl_is_null,
            dacl_protected,
            acl_valid,
            aces,
        }
    }
    /// Returns the owner SID in string form.
    pub fn owner_sid(&self) -> &str {
        &self.owner_sid
    }
    /// Returns whether the owner SID matches the current security principal.
    pub const fn owner_is_current_user(&self) -> bool {
        self.owner_is_current_user
    }
    /// Returns whether the security descriptor has a DACL.
    pub const fn dacl_present(&self) -> bool {
        self.dacl_present
    }
    /// Returns whether the descriptor contains a NULL DACL.
    pub const fn dacl_is_null(&self) -> bool {
        self.dacl_is_null
    }
    /// Returns whether DACL inheritance is protected.
    pub const fn dacl_protected(&self) -> bool {
        self.dacl_protected
    }
    /// Returns whether the DACL structure passed validation.
    pub const fn acl_valid(&self) -> bool {
        self.acl_valid
    }
    /// Returns ACEs in native descriptor order.
    pub fn aces(&self) -> &[LocalWindowsAceObservation] {
        &self.aces
    }
}
