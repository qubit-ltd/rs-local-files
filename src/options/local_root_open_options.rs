use super::LocalResolutionLimits;
use crate::directory::LocalResolutionPolicy;
use crate::policy::LocalSymlinkPolicy;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
#[must_use]
/// Configuration for opening a rooted filesystem with additional resolution rules.
pub struct LocalRootOpenOptions {
    limits: LocalResolutionLimits,
    symlink: Option<LocalSymlinkPolicy>,
    policy: Option<Arc<dyn LocalResolutionPolicy>>,
}
impl LocalRootOpenOptions {
    /// Creates options with no caller-added resolution limits or policy.
    pub fn new() -> Self {
        Self::default()
    }
    /// Sets per-resolution limits; zero is a real limit and `None` is unlimited.
    pub fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.limits = limits;
        self
    }
    /// Sets symlink handling for opening the root and subsequent rooted operations.
    pub fn with_symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink = Some(policy);
        self
    }
    /// Adds a pure component and symlink-target validation policy.
    pub fn with_resolution_policy(mut self, policy: Arc<dyn LocalResolutionPolicy>) -> Self {
        self.policy = Some(policy);
        self
    }
    pub(crate) const fn limits(&self) -> LocalResolutionLimits {
        self.limits
    }
    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink
    }
    pub(crate) fn resolution_policy(&self) -> Option<&Arc<dyn LocalResolutionPolicy>> {
        self.policy.as_ref()
    }
}
