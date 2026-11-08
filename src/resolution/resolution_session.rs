// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::path::Path;

use super::ResolutionContract;
use super::session::limit_error;
use super::session::limit_error_with_limit;
use crate::LocalResult;
use crate::directory::LocalResolutionOrigin;
use crate::directory::LocalResolutionScope;
use crate::error::LocalResourceKind;

/// Measures an operating-system string in the native path-byte unit.
///
/// # Parameters
///
/// * `value` - String whose native path representation is measured.
///
/// # Returns
///
/// `Some` contains the byte count; `None` means the platform representation
/// cannot be measured without overflow or loss of native encoding.
fn native_bytes(value: &OsStr) -> Option<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(value.as_bytes().len())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().count().checked_mul(2)
    }
    #[cfg(not(any(unix, windows)))]
    {
        value.to_str()?.len().checked_mul(4)
    }
}

/// Tracks resource limits and component policy for one path-resolution attempt.
///
/// Hop counts include each link before following it. Pending counts describe
/// the unresolved worklist; resolved counts describe the accumulated result.
/// Path-byte limits use native encoded bytes. The inherited contract supplies
/// the strictest limits and policies; the resolver checks its deadline
/// separately.
///
/// The resolver creates one session per attempt and charges each link before
/// traversal.
#[must_use]
pub(crate) struct ResolutionSession {
    /// Effective inherited limits and component policies for this operation.
    contract: ResolutionContract,
    /// Scope used when evaluating component restrictions.
    scope: LocalResolutionScope,
    /// Number of links charged before following them.
    hops: usize,
}

impl ResolutionSession {
    /// Starts a resolution session with zero followed links.
    ///
    /// # Parameters
    ///
    /// * `contract` - Effective inherited policy to snapshot for this attempt.
    /// * `scope` - Scope used to evaluate component restrictions.
    ///
    /// # Returns
    ///
    /// A session with the contract snapshot and a zero hop count.
    pub(crate) fn new(contract: &ResolutionContract, scope: LocalResolutionScope) -> Self {
        Self {
            contract: contract.clone(),
            scope,
            hops: 0,
        }
    }

    /// Charges one hop before following a link and enforces the inherited cap.
    ///
    /// Returns a resource-limit error on counter overflow or when the updated
    /// count exceeds the contract. Component policies are checked separately
    /// as each link target is interpreted.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error when the counter overflows or exceeds its
    /// cap.
    ///
    /// # Returns
    ///
    /// Returns unit after charging an allowed hop.
    pub(crate) fn before_follow_link(&mut self) -> LocalResult<()> {
        self.hops = self
            .hops
            .checked_add(1)
            .ok_or_else(|| limit_error(LocalResourceKind::SymlinkHops, usize::MAX, true))?;
        if let Some(limit) = self.contract.limits().symlink_hops()
            && self.hops > limit
        {
            return Err(limit_error_with_limit(
                LocalResourceKind::SymlinkHops,
                limit,
                self.hops,
                false,
            ));
        }
        Ok(())
    }

    /// Checks the unresolved worklist against component and native-byte limits.
    ///
    /// # Parameters
    ///
    /// * `components` - Number of unresolved path components.
    /// * `bytes` - Native encoded byte length of the unresolved path.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error when either observed value exceeds its
    /// cap.
    ///
    /// # Returns
    ///
    /// Returns unit when both pending measurements satisfy their limits.
    pub(crate) fn check_pending(&self, components: usize, bytes: usize) -> LocalResult<()> {
        self.check(
            LocalResourceKind::PendingResolutionComponents,
            components,
            self.contract.limits().actual_components(),
        )?;
        self.check(
            LocalResourceKind::PendingResolutionBytes,
            bytes,
            self.contract.limits().resolved_path_bytes(),
        )
    }

    /// Checks the accumulated path against component and native-byte limits.
    ///
    /// This is independent of [`Self::check_pending`]; both snapshots must fit
    /// their respective limits during resolution.
    ///
    /// # Parameters
    ///
    /// * `components` - Number of accumulated resolved components.
    /// * `bytes` - Native encoded byte length of the accumulated path.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error when either observed value exceeds its
    /// cap.
    ///
    /// # Returns
    ///
    /// Returns unit when both resolved measurements satisfy their limits.
    pub(crate) fn check_resolved(&self, components: usize, bytes: usize) -> LocalResult<()> {
        self.check(
            LocalResourceKind::ResolvedComponents,
            components,
            self.contract.limits().actual_components(),
        )?;
        self.check(
            LocalResourceKind::ResolvedPathBytes,
            bytes,
            self.contract.limits().resolved_path_bytes(),
        )
    }

    /// Applies the inherited component policy in the supplied resolution scope.
    ///
    /// # Parameters
    ///
    /// * `component` - Component to check without converting its native form.
    /// * `origin` - Resolution source used to select the applicable policy.
    ///
    /// # Errors
    ///
    /// Returns the contract's typed error when the component is denied.
    ///
    /// # Returns
    ///
    /// Returns unit when the component is permitted.
    pub(crate) fn validate_component(&self, component: &OsStr, origin: LocalResolutionOrigin) -> LocalResult<()> {
        self.contract.validate_component(component, self.scope, origin)
    }

    /// Checks target byte length before applying target component policy.
    ///
    /// Windows measures UTF-16 code units times two. Overflow and policy
    /// rejection remain resource-limit and typed contract errors respectively.
    ///
    /// # Parameters
    ///
    /// * `target` - Link target to validate using native path semantics.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error for an oversized target, or the
    /// contract's typed error when target policy rejects it.
    ///
    /// # Returns
    ///
    /// Returns unit when the target fits its byte limit and policy.
    pub(crate) fn validate_target(&self, target: &Path) -> LocalResult<()> {
        let bytes = native_bytes(target.as_os_str())
            .ok_or_else(|| limit_error(LocalResourceKind::SymlinkTargetBytes, usize::MAX, true))?;
        self.check(
            LocalResourceKind::SymlinkTargetBytes,
            bytes,
            self.contract.limits().symlink_target_bytes(),
        )?;
        self.contract.validate_target(target, self.scope)
    }

    /// Returns a resource-limit error when an observed value exceeds its cap.
    ///
    /// # Parameters
    ///
    /// * `kind` - Resource category associated with the observation.
    /// * `observed` - Measured resource usage.
    /// * `limit` - Optional maximum usage; `None` means unbounded.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error only when `observed` exceeds `limit`.
    ///
    /// # Returns
    ///
    /// Returns unit when the observed value is within the optional limit.
    fn check(&self, kind: LocalResourceKind, observed: usize, limit: Option<usize>) -> LocalResult<()> {
        if let Some(limit) = limit
            && observed > limit
        {
            return Err(limit_error_with_limit(kind, limit, observed, false));
        }
        Ok(())
    }
}

#[cfg(test)]
mod overflow_contract_tests {
    use super::ResolutionSession;
    use crate::directory::LocalResolutionScope;
    use crate::error::LocalFileOperation;
    use crate::error::LocalResourceKind;
    use crate::resolution::ResolutionContract;

    #[test]
    fn test_hop_overflow_retains_counter_and_reports_unrepresentable_observation() {
        let mut session = ResolutionSession::new(&ResolutionContract::unrestricted(), LocalResolutionScope::Descendant);
        session.hops = usize::MAX;

        let error = session
            .before_follow_link()
            .expect_err("overflow must reject another symlink hop");

        assert_eq!(session.hops, usize::MAX);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        let facts = error
            .resource_limit_error()
            .expect("hop overflow must retain budget facts");
        assert_eq!(facts.resource(), LocalResourceKind::SymlinkHops);
        assert_eq!(facts.limit(), usize::MAX);
        assert_eq!(facts.remaining(), 0);
        assert_eq!(facts.requested(), 1);
        assert_eq!(facts.observed(), None);
        assert!(facts.overflowed());
    }
}
