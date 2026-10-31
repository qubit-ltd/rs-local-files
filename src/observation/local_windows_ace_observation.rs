// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// One Windows access-control entry, retaining undecoded native facts.
///
/// The type and flag bytes are the native ACE values. Consumers needing an
/// access decision must interpret them together with the descriptor and the
/// operating system's access-check rules.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::LocalWindowsAceObservation;
///
/// fn inspect(entry: &LocalWindowsAceObservation) -> (u8, Option<u32>) {
///     (entry.ace_type(), entry.access_mask())
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalWindowsAceObservation {
    /// Native ACE type byte; its interpretation depends on the descriptor.
    ace_type: u8,
    /// Native ACE inheritance and audit flags.
    flags: u8,
    /// Native size of this ACE in bytes.
    native_size: u16,
    /// Decoded access mask, absent for ACE forms without a mask.
    access_mask: Option<u32>,
    /// Decoded trustee SID, absent when no trustee is exposed or decoding failed.
    trustee_sid: Option<String>,
    /// Whether the decoded trustee matches the current principal, if knowable.
    trustee_is_current_user: Option<bool>,
    /// Whether the supported ACE type and trustee facts were fully decoded.
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

#[cfg(test)]
mod tests {
    use super::LocalWindowsAceObservation;

    #[test]
    fn test_windows_ace_facts_are_readable_on_every_platform() {
        let decoded = LocalWindowsAceObservation {
            ace_type: 0,
            flags: 0x12,
            native_size: 24,
            access_mask: Some(0x001f_01ff),
            trustee_sid: Some("S-1-5-21-1000".to_owned()),
            trustee_is_current_user: Some(false),
            is_decoded: true,
        };
        assert_eq!(decoded.ace_type(), 0);
        assert_eq!(decoded.flags(), 0x12);
        assert_eq!(decoded.native_size(), 24);
        assert_eq!(decoded.access_mask(), Some(0x001f_01ff));
        assert_eq!(decoded.trustee_sid(), Some("S-1-5-21-1000"));
        assert_eq!(decoded.trustee_is_current_user(), Some(false));
        assert!(decoded.is_decoded());

        let undecoded = LocalWindowsAceObservation {
            ace_type: 0x7f,
            flags: 0x80,
            native_size: 4,
            access_mask: None,
            trustee_sid: None,
            trustee_is_current_user: None,
            is_decoded: false,
        };
        assert_eq!(undecoded.ace_type(), 0x7f);
        assert_eq!(undecoded.flags(), 0x80);
        assert_eq!(undecoded.native_size(), 4);
        assert_eq!(undecoded.access_mask(), None);
        assert_eq!(undecoded.trustee_sid(), None);
        assert_eq!(undecoded.trustee_is_current_user(), None);
        assert!(!undecoded.is_decoded());
    }
}
