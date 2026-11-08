// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalWindowsAceObservation;

/// Windows owner, DACL state, and ordered ACE observations.
///
/// `owner_is_current_user` compares the owner SID with the effective principal
/// at query time. Windows first uses the calling thread token, falling back to
/// the process token only when the thread has no token (`ERROR_NO_TOKEN`). This
/// comparison is an observation, not an access authorization decision.
///
/// The DACL flags distinguish an absent DACL (`dacl_present() == false`), a
/// NULL DACL (`dacl_present() == true` and `dacl_is_null() == true`), and an
/// empty ACL (`dacl_present() == true`, `dacl_is_null() == false`, and no
/// ACEs). `dacl_protected()` reports whether inheritance is protected, while
/// `acl_valid()` reports whether the ACL structure passed validation. ACEs are
/// retained in native descriptor order; they are not reordered into an
/// authorization result.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::{
///     LocalObservedReader, LocalObservationRequirement,
///     LocalSecurityObservationOptions,
/// };
///
/// fn inspect(reader: &LocalObservedReader) -> Result<(), qubit_local_files::error::LocalFileError> {
///     let options = LocalSecurityObservationOptions::new()
///         .with_access(LocalObservationRequirement::Optional);
///     let facts = reader.security_observation(&options)?;
///     let Some(access) = facts.access().and_then(|access| access.windows()) else {
///         // Optional access facts can be unavailable on this platform.
///         return Ok(());
///     };
///     let _owner_sid = access.owner_sid();
///     let _owner_matches_query_principal = access.owner_is_current_user();
///     let _dacl_state = (
///         access.dacl_present(),
///         access.dacl_is_null(),
///         access.dacl_protected(),
///         access.acl_valid(),
///     );
///     let _ordered_aces = access.aces();
///     Ok(())
/// }
/// # fn main() {}
/// ```
///
/// Optional requirements still attempt the native query. Unsupported facts
/// may be absent, while native query errors are propagated to the caller.
/// The fields describe observations, not a guarantee of future access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalWindowsAccessObservation {
    /// Owner SID observed in the security descriptor, in string form.
    owner_sid: String,
    /// Whether the owner SID matched the effective principal at query time.
    owner_is_current_user: bool,
    /// Whether the security descriptor contains a DACL.
    dacl_present: bool,
    /// Whether a present DACL is NULL rather than an empty ACL.
    dacl_is_null: bool,
    /// Whether inheritance is protected on the observed DACL.
    dacl_protected: bool,
    /// Whether the observed ACL structure passed validation.
    acl_valid: bool,
    /// ACE observations in native descriptor order.
    aces: Vec<LocalWindowsAceObservation>,
}
impl LocalWindowsAccessObservation {
    /// Builds access facts from a Windows security descriptor query.
    ///
    /// The supplied values retain the query's observations; they do not
    /// represent an authorization decision or guarantee future access.
    ///
    /// # Parameters
    ///
    /// - `owner_sid`: Owner SID in string form.
    /// - `owner_is_current_user`: Whether it matched the effective principal.
    /// - `dacl_present`: Whether the descriptor contains a DACL.
    /// - `dacl_is_null`: Whether the present DACL is NULL.
    /// - `dacl_protected`: Whether DACL inheritance is protected.
    /// - `acl_valid`: Whether the ACL structure passed validation.
    /// - `aces`: ACE facts in native descriptor order.
    ///
    /// # Returns
    ///
    /// An observation containing the supplied descriptor facts.
    #[cfg(windows)]
    #[allow(dead_code)] // Consumed when T04 connects Windows security queries.
    #[must_use]
    #[inline]
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
    #[must_use]
    #[inline]
    pub fn owner_sid(&self) -> &str {
        &self.owner_sid
    }
    /// Returns whether the owner SID matched the effective principal at query
    /// time (thread token, or process token only when no thread token exists).
    /// This is not an access authorization result.
    #[must_use]
    #[inline]
    pub const fn owner_is_current_user(&self) -> bool {
        self.owner_is_current_user
    }
    /// Returns whether the security descriptor has a DACL. `false`
    /// distinguishes an absent DACL from both a NULL DACL and an empty ACL.
    #[must_use]
    #[inline]
    pub const fn dacl_present(&self) -> bool {
        self.dacl_present
    }
    /// Returns whether a present DACL is NULL. A NULL DACL is distinct from an
    /// absent DACL and from a present, empty ACL.
    #[must_use]
    #[inline]
    pub const fn dacl_is_null(&self) -> bool {
        self.dacl_is_null
    }
    /// Returns whether DACL inheritance is protected; this is independent of
    /// whether the ACL structure is valid.
    #[must_use]
    #[inline]
    pub const fn dacl_protected(&self) -> bool {
        self.dacl_protected
    }
    /// Returns whether the DACL structure passed validation. This reports
    /// structural validity, not the result of evaluating ACE permissions.
    #[must_use]
    #[inline]
    pub const fn acl_valid(&self) -> bool {
        self.acl_valid
    }
    /// Returns ACEs in native descriptor order.
    #[must_use]
    #[inline]
    pub fn aces(&self) -> &[LocalWindowsAceObservation] {
        &self.aces
    }
}

#[cfg(test)]
mod tests {
    use super::LocalWindowsAccessObservation;

    #[test]
    fn test_windows_dacl_states_are_readable_on_every_platform() {
        for (present, null, protected, valid, current_owner) in [
            (false, false, false, false, false),
            (true, true, false, true, true),
            (true, false, true, true, false),
        ] {
            let observation = LocalWindowsAccessObservation {
                owner_sid: "S-1-5-18".to_owned(),
                owner_is_current_user: current_owner,
                dacl_present: present,
                dacl_is_null: null,
                dacl_protected: protected,
                acl_valid: valid,
                aces: Vec::new(),
            };
            assert_eq!(observation.owner_sid(), "S-1-5-18");
            assert_eq!(observation.owner_is_current_user(), current_owner);
            assert_eq!(observation.dacl_present(), present);
            assert_eq!(observation.dacl_is_null(), null);
            assert_eq!(observation.dacl_protected(), protected);
            assert_eq!(observation.acl_valid(), valid);
            assert!(observation.aces().is_empty());
        }
    }
}
