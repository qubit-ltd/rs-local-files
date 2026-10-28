// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use crate::directory::LocalResolutionOrigin;
use crate::directory::LocalResolutionPolicy;
use crate::directory::LocalResolutionScope;
use crate::options::LocalResolutionLimits;

#[derive(Clone)]
pub(crate) struct ResolutionContract {
    limits: LocalResolutionLimits,
    policies: Vec<Arc<dyn LocalResolutionPolicy>>,
}
impl ResolutionContract {
    pub(crate) fn limits(&self) -> LocalResolutionLimits {
        self.limits
    }
    pub(crate) fn is_unrestricted(&self) -> bool {
        self.limits == LocalResolutionLimits::new() && self.policies.is_empty()
    }
    pub(crate) fn validate_component(
        &self,
        component: &std::ffi::OsStr,
        scope: LocalResolutionScope,
        origin: LocalResolutionOrigin,
    ) -> crate::LocalResult<()> {
        for policy in &self.policies {
            policy.validate_component(component, scope, origin)?;
        }
        Ok(())
    }
    pub(crate) fn validate_target(
        &self,
        target: &std::path::Path,
        scope: LocalResolutionScope,
    ) -> crate::LocalResult<()> {
        for policy in &self.policies {
            policy.validate_symlink_target(target, scope)?;
        }
        Ok(())
    }
    pub(crate) fn unrestricted() -> Self {
        Self {
            limits: LocalResolutionLimits::new(),
            policies: Vec::new(),
        }
    }
    pub(crate) fn tightened(&self, limits: LocalResolutionLimits) -> Self {
        Self {
            limits: self.limits.tightened(limits),
            policies: self.policies.clone(),
        }
    }
    pub(crate) fn with_policy(&self, policy: Arc<dyn LocalResolutionPolicy>) -> Self {
        let mut policies = self.policies.clone();
        policies.push(policy);
        Self {
            limits: self.limits,
            policies,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex;

    use super::ResolutionContract;
    use crate::LocalResult;
    use crate::directory::LocalResolutionOrigin;
    use crate::directory::LocalResolutionPolicy;
    use crate::directory::LocalResolutionScope;
    use crate::options::LocalResolutionLimits;

    #[derive(Debug)]
    struct Recorder(&'static str, Arc<Mutex<Vec<&'static str>>>);
    impl LocalResolutionPolicy for Recorder {
        fn validate_component(&self, _: &OsStr, _: LocalResolutionScope, _: LocalResolutionOrigin) -> LocalResult<()> {
            self.1.lock().unwrap().push(self.0);
            Ok(())
        }
        fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> LocalResult<()> {
            Ok(())
        }
    }

    #[test]
    fn resolution_session_policy_chain_runs_parent_then_child() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let parent = ResolutionContract::unrestricted().with_policy(Arc::new(Recorder("parent", calls.clone())));
        let child = parent.with_policy(Arc::new(Recorder("child", calls.clone())));
        child
            .validate_component(
                OsStr::new("x"),
                LocalResolutionScope::Descendant,
                LocalResolutionOrigin::Input,
            )
            .unwrap();
        assert_eq!(*calls.lock().unwrap(), ["parent", "child"]);
    }

    #[test]
    fn resolution_session_contract_is_unrestricted_only_without_limits_or_policies() {
        let unrestricted = ResolutionContract::unrestricted();
        assert!(unrestricted.is_unrestricted());
        let limited = unrestricted.tightened(LocalResolutionLimits::new().with_max_symlink_hops(1));
        assert!(!limited.is_unrestricted());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let policy = ResolutionContract::unrestricted().with_policy(Arc::new(Recorder("policy", calls)));
        assert!(!policy.is_unrestricted());
    }

    fn values(limits: LocalResolutionLimits) -> [Option<usize>; 4] {
        [
            limits.symlink_hops(),
            limits.symlink_target_bytes(),
            limits.actual_components(),
            limits.resolved_path_bytes(),
        ]
    }

    #[test]
    fn resolution_contract_child_none_preserves_each_parent_limit() {
        let parent = LocalResolutionLimits::new()
            .with_max_symlink_hops(1)
            .with_max_symlink_target_bytes(2)
            .with_max_actual_components(3)
            .with_max_resolved_path_bytes(4);
        let child = LocalResolutionLimits::new();

        assert_eq!(
            values(
                ResolutionContract::unrestricted()
                    .tightened(parent)
                    .tightened(child)
                    .limits
            ),
            [Some(1), Some(2), Some(3), Some(4)]
        );
    }

    #[test]
    fn resolution_contract_parent_none_adopts_each_child_limit() {
        let child = LocalResolutionLimits::new()
            .with_max_symlink_hops(5)
            .with_max_symlink_target_bytes(6)
            .with_max_actual_components(7)
            .with_max_resolved_path_bytes(8);

        assert_eq!(
            values(ResolutionContract::unrestricted().tightened(child).limits),
            [Some(5), Some(6), Some(7), Some(8)]
        );
    }

    #[test]
    fn resolution_contract_uses_the_smaller_of_each_pair() {
        let parent = LocalResolutionLimits::new()
            .with_max_symlink_hops(8)
            .with_max_symlink_target_bytes(1)
            .with_max_actual_components(6)
            .with_max_resolved_path_bytes(9);
        let child = LocalResolutionLimits::new()
            .with_max_symlink_hops(4)
            .with_max_symlink_target_bytes(9)
            .with_max_actual_components(3)
            .with_max_resolved_path_bytes(2);

        assert_eq!(
            values(
                ResolutionContract::unrestricted()
                    .tightened(parent)
                    .tightened(child)
                    .limits
            ),
            [Some(4), Some(1), Some(3), Some(2)]
        );
    }

    #[test]
    fn resolution_contract_keeps_empty_limits_unrestricted() {
        let empty = LocalResolutionLimits::new();

        assert_eq!(
            values(ResolutionContract::unrestricted().tightened(empty).limits),
            [None; 4]
        );
    }

    #[test]
    fn resolution_contract_preserves_zero_as_a_real_limit() {
        let parent = LocalResolutionLimits::new().with_max_symlink_hops(0);
        let child = LocalResolutionLimits::new().with_max_symlink_hops(3);

        assert_eq!(
            values(
                ResolutionContract::unrestricted()
                    .tightened(parent)
                    .tightened(child)
                    .limits
            ),
            [Some(0), None, None, None]
        );
    }
}
pub(crate) mod native_tokens;
pub(crate) mod session;
