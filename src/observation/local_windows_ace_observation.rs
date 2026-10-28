// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
/// One Windows access-control entry, retaining undecoded native facts.
///
/// The type and flag bytes are the native ACE values. Consumers needing an
/// access decision must interpret them together with the descriptor and the
/// operating system's access-check rules.
pub struct LocalWindowsAceObservation {
    ace_type: u8,
    flags: u8,
    native_size: u16,
    access_mask: Option<u32>,
    trustee_sid: Option<String>,
    trustee_is_current_user: Option<bool>,
    is_decoded: bool,
}
impl LocalWindowsAceObservation {
    #[cfg(windows)]
    #[allow(dead_code)] // Constructed while T04 decodes Windows ACEs.
    pub(crate) const fn new(
        ace_type: u8,
        flags: u8,
        native_size: u16,
        access_mask: Option<u32>,
        trustee_sid: Option<String>,
        trustee_is_current_user: Option<bool>,
        is_decoded: bool,
    ) -> Self {
        Self {
            ace_type,
            flags,
            native_size,
            access_mask,
            trustee_sid,
            trustee_is_current_user,
            is_decoded,
        }
    }
    /// Returns the native ACE type byte.
    pub const fn ace_type(&self) -> u8 {
        self.ace_type
    }
    /// Returns the native ACE flags byte.
    pub const fn flags(&self) -> u8 {
        self.flags
    }
    /// Returns the native ACE size in bytes.
    pub const fn native_size(&self) -> u16 {
        self.native_size
    }
    /// Returns a decoded access mask when the ACE type exposes one, or `None`
    /// for ACE forms without an access mask.
    pub const fn access_mask(&self) -> Option<u32> {
        self.access_mask
    }
    /// Returns the trustee SID when it was decoded, or `None` when the ACE
    /// form has no trustee SID or decoding was unavailable.
    pub fn trustee_sid(&self) -> Option<&str> {
        self.trustee_sid.as_deref()
    }
    /// Returns whether the trustee is the current principal, or `None` when
    /// the trustee could not be compared.
    pub const fn trustee_is_current_user(&self) -> Option<bool> {
        self.trustee_is_current_user
    }
    /// Returns whether the ACE type and trustee were fully decoded.
    pub const fn is_decoded(&self) -> bool {
        self.is_decoded
    }
}
