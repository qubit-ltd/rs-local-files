use super::LocalReadOptions;
use crate::policy::LocalSymlinkPolicy;

/// Options for bounded complete-file reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "read-all options have no effect unless they are used"]
pub struct LocalReadAllOptions {
    max_bytes: Option<u64>,
    read_options: Option<LocalReadOptions>,
    symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalReadAllOptions {
    /// Creates options that inherit filesystem reader defaults and have no
    /// additional byte limit.
    pub const fn new() -> Self {
        Self {
            max_bytes: None,
            read_options: None,
            symlink_policy: None,
        }
    }

    /// Sets the maximum number of bytes returned by a complete read.
    pub const fn with_max_bytes(mut self, maximum: u64) -> Self {
        self.max_bytes = Some(maximum);
        self
    }

    /// Replaces the filesystem's complete reader options.
    pub const fn with_read_options(mut self, options: LocalReadOptions) -> Self {
        self.read_options = Some(options);
        self
    }

    /// Overrides the symbolic-link policy after selecting reader options.
    pub const fn with_symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(policy);
        self
    }

    pub(crate) const fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }

    pub(crate) const fn read_options(&self) -> Option<&LocalReadOptions> {
        self.read_options.as_ref()
    }

    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }
}
