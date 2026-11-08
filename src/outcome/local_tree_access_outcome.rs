// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::outcome::LocalTreeAccessState;
use crate::policy::LocalOwnerAccessPolicy;
/// Reports the access policy confirmed for a staged tree.
///
/// The access state describes the result of that access operation, while
/// `owner_access_policy` is `Some` only when an exact owner policy was applied
/// and verified. A preserved state does not imply that a requested mutation
/// occurred.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::{LocalDirectoryOpenOptions, LocalTempDirectoryOptions};
/// use qubit_local_files::options::LocalListOptions;
/// use qubit_local_files::policy::{LocalOwnerAccessPolicy, LocalTreeFinalAccess};
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let temporary = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let parent = filesystem.open_directory_with_options(
///     temporary.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let mut stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// #[cfg(target_os = "linux")]
/// {
///     let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
///     let outcome = stage.apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())?;
///     assert_eq!(outcome.access_state(), qubit_local_files::outcome::LocalTreeAccessState::OwnerPolicyApplied);
///     assert_eq!(outcome.owner_access_policy(), Some(&policy));
/// }
/// #[cfg(not(target_os = "linux"))]
/// {
///     let outcome = stage.apply_access_policy(LocalTreeFinalAccess::Preserve)?;
///     assert_eq!(outcome.access_state(), qubit_local_files::outcome::LocalTreeAccessState::Preserved);
///     assert!(outcome.owner_access_policy().is_none());
/// }
/// stage.cleanup()?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalTreeAccessOutcome {
    /// State confirmed for the staged tree after the access operation.
    state: LocalTreeAccessState,
    /// Exact policy applied and verified, or `None` when no owner policy was
    /// applied or its exact result is not available.
    owner_access_policy: Option<LocalOwnerAccessPolicy>,
}
impl LocalTreeAccessOutcome {
    /// Creates an outcome without a confirmed exact owner policy.
    ///
    /// This constructor is used when the operation preserves access or reports
    /// a state that does not correspond to a verified owner policy.
    ///
    /// # Parameters
    ///
    /// - `state`: The access state confirmed for the staged tree.
    #[must_use]
    #[inline]
    pub(crate) const fn new(state: LocalTreeAccessState) -> Self {
        Self {
            state,
            owner_access_policy: None,
        }
    }

    /// Creates an outcome for an exact owner policy that was applied and
    /// verified.
    ///
    /// The supplied policy is retained so callers can distinguish a verified
    /// configuration from a general access state.
    ///
    /// # Parameters
    ///
    /// - `state`: The access state confirmed for the staged tree.
    /// - `policy`: The exact owner policy applied and verified for the tree.
    #[must_use]
    #[inline]
    pub(crate) const fn with_owner_policy(state: LocalTreeAccessState, policy: LocalOwnerAccessPolicy) -> Self {
        Self {
            state,
            owner_access_policy: Some(policy),
        }
    }
    /// Returns the access state confirmed by the operation.
    ///
    /// This reports the observed result and does not imply that a requested
    /// mutation occurred when the state is [`LocalTreeAccessState::Preserved`].
    ///
    /// # Returns
    ///
    /// The access state observed and confirmed for the staged tree.
    #[must_use]
    #[inline]
    pub const fn access_state(self) -> LocalTreeAccessState {
        self.state
    }
    /// Returns the exact owner policy confirmed for this tree, when applied.
    ///
    /// Returns `Some` only when the exact policy was applied and verified;
    /// otherwise returns `None`. The reference borrows the stored policy from
    /// this outcome.
    ///
    /// # Returns
    ///
    /// `Some` with a reference to the verified policy, or `None` when no exact
    /// owner policy was applied and verified.
    #[must_use = "inspect the returned policy to confirm the applied configuration"]
    #[inline]
    pub const fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
}
