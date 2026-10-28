use super::ResolutionContract;
use crate::{
    directory::{LocalResolutionOrigin, LocalResolutionScope},
    error::{LocalFileError, LocalFileOperation, LocalResourceKind, LocalResourceLimitError},
    LocalResult,
};
use std::{ffi::OsStr, path::Path};

/// Counters and validation policy for one path-resolution attempt.
pub(crate) struct ResolutionSession {
    contract: ResolutionContract,
    scope: LocalResolutionScope,
    hops: usize,
}

impl ResolutionSession {
    pub(crate) fn new(contract: &ResolutionContract, scope: LocalResolutionScope) -> Self {
        Self {
            contract: contract.clone(),
            scope,
            hops: 0,
        }
    }

    pub(crate) fn before_follow_link(&mut self) -> LocalResult<()> {
        self.hops = self
            .hops
            .checked_add(1)
            .ok_or_else(|| limit_error(LocalResourceKind::SymlinkHops, usize::MAX, true))?;
        if let Some(limit) = self.contract.limits().symlink_hops() {
            if self.hops > limit {
                return Err(limit_error_with_limit(
                    LocalResourceKind::SymlinkHops,
                    limit,
                    self.hops,
                    false,
                ));
            }
        }
        Ok(())
    }

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

    pub(crate) fn validate_component(
        &self,
        component: &OsStr,
        origin: LocalResolutionOrigin,
    ) -> LocalResult<()> {
        self.contract
            .validate_component(component, self.scope, origin)
    }

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

    fn check(
        &self,
        kind: LocalResourceKind,
        observed: usize,
        limit: Option<usize>,
    ) -> LocalResult<()> {
        if let Some(limit) = limit {
            if observed > limit {
                return Err(limit_error_with_limit(kind, limit, observed, false));
            }
        }
        Ok(())
    }
}

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

pub(crate) fn overflow_error(kind: LocalResourceKind) -> LocalFileError {
    limit_error(kind, usize::MAX, true)
}

fn limit_error(kind: LocalResourceKind, observed: usize, overflow: bool) -> LocalFileError {
    limit_error_with_limit(kind, usize::MAX, observed, overflow)
}
fn limit_error_with_limit(
    kind: LocalResourceKind,
    limit: usize,
    observed: usize,
    overflow: bool,
) -> LocalFileError {
    let error =
        LocalResourceLimitError::new(kind, limit, limit.saturating_sub(observed.min(limit)), 1);
    let error = if overflow {
        error.with_overflow()
    } else {
        error.with_observed(observed)
    };
    LocalFileError::from_resource_limit(LocalFileOperation::Metadata, None, error)
}

#[cfg(test)]
mod tests {
    use super::ResolutionSession;
    use crate::{
        directory::LocalResolutionScope,
        error::{LocalFileErrorKind, LocalResourceKind},
        options::LocalResolutionLimits,
        resolution::ResolutionContract,
    };

    #[test]
    fn resolution_session_zero_forbids_following_links() {
        let contract = ResolutionContract::unrestricted()
            .tightened(LocalResolutionLimits::new().with_max_symlink_hops(0));
        let mut session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
        let error = session.before_follow_link().unwrap_err();
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        let facts = error.resource_limit_error().unwrap();
        assert_eq!(facts.resource(), LocalResourceKind::SymlinkHops);
        assert_eq!(facts.limit(), 0);
        assert_eq!(facts.remaining(), 0);
        assert_eq!(facts.requested(), 1);
        assert_eq!(facts.observed(), Some(1));
        assert!(!facts.overflowed());
    }

    #[test]
    fn resolution_session_accepts_exact_hop_limit_and_rejects_next_hop() {
        let contract = ResolutionContract::unrestricted()
            .tightened(LocalResolutionLimits::new().with_max_symlink_hops(1));
        let mut session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
        session.before_follow_link().unwrap();
        assert_eq!(
            session
                .before_follow_link()
                .unwrap_err()
                .resource_limit_error()
                .unwrap()
                .observed(),
            Some(2)
        );
    }

    #[test]
    fn resolution_session_checks_pending_and_resolved_snapshots_independently() {
        let contract = ResolutionContract::unrestricted().tightened(
            LocalResolutionLimits::new()
                .with_max_actual_components(2)
                .with_max_resolved_path_bytes(3),
        );
        let session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
        session.check_pending(2, 3).unwrap();
        session.check_resolved(2, 3).unwrap();
        assert_eq!(
            session
                .check_pending(3, 3)
                .unwrap_err()
                .resource_limit_error()
                .unwrap()
                .resource(),
            LocalResourceKind::PendingResolutionComponents
        );
        assert_eq!(
            session
                .check_resolved(2, 4)
                .unwrap_err()
                .resource_limit_error()
                .unwrap()
                .resource(),
            LocalResourceKind::ResolvedPathBytes
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolution_session_target_byte_limit_accepts_l_and_rejects_l_plus_one() {
        use std::path::Path;
        let contract = ResolutionContract::unrestricted()
            .tightened(LocalResolutionLimits::new().with_max_symlink_target_bytes(3));
        let session = ResolutionSession::new(&contract, LocalResolutionScope::Descendant);
        session.validate_target(Path::new("abc")).unwrap();
        let smaller = ResolutionContract::unrestricted()
            .tightened(LocalResolutionLimits::new().with_max_symlink_target_bytes(2));
        let error = ResolutionSession::new(&smaller, LocalResolutionScope::Descendant)
            .validate_target(Path::new("abc"))
            .unwrap_err();
        let facts = error.resource_limit_error().unwrap();
        assert_eq!(facts.resource(), LocalResourceKind::SymlinkTargetBytes);
        assert_eq!(facts.observed(), Some(3));
    }
}
