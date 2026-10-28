use crate::{directory::LocalResolutionPolicy, options::LocalResolutionLimits};
use std::sync::Arc;

pub(crate) struct ResolutionContract {
    limits: LocalResolutionLimits,
    policies: Vec<Arc<dyn LocalResolutionPolicy>>,
}
impl ResolutionContract {
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
    use super::ResolutionContract;
    use crate::options::LocalResolutionLimits;

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
