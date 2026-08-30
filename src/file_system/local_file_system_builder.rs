//! Builder for immutable Host and Rooted filesystem instances.

use std::path::Path;
use std::path::PathBuf;

use super::LocalCopyLimits;
use super::LocalWalkLimits;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;

enum AuthoritySpec {
    Host,
    Rooted(PathBuf),
}

/// Builder for a Host or handle-confined Rooted filesystem.
pub struct LocalFileSystemBuilder {
    authority: AuthoritySpec,
    symlink_policy: LocalSymlinkPolicy,
    walk_limits: LocalWalkLimits,
    copy_limits: LocalCopyLimits,
    #[cfg(feature = "test-support")]
    test_faults: Option<crate::TestFaultPlan>,
}

impl LocalFileSystemBuilder {
    /// Starts a Host filesystem builder.
    pub fn host() -> Self {
        Self {
            authority: AuthoritySpec::Host,
            symlink_policy: LocalSymlinkPolicy::FollowAcrossScope,
            walk_limits: LocalWalkLimits::default(),
            copy_limits: LocalCopyLimits::default(),
            #[cfg(feature = "test-support")]
            test_faults: None,
        }
    }

    /// Starts a Rooted filesystem builder.
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        Self {
            authority: AuthoritySpec::Rooted(root.as_ref().to_path_buf()),
            symlink_policy: LocalSymlinkPolicy::FollowWithinScope,
            walk_limits: LocalWalkLimits::default(),
            copy_limits: LocalCopyLimits::default(),
            #[cfg(feature = "test-support")]
            test_faults: None,
        }
    }

    /// Sets the symbolic-link policy used by the built instance.
    pub const fn symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = policy;
        self
    }

    /// Sets traversal limits for the built instance.
    pub const fn walk_limits(mut self, limits: LocalWalkLimits) -> Self {
        self.walk_limits = limits;
        self
    }

    /// Sets copy limits for the built instance.
    pub const fn copy_limits(mut self, limits: LocalCopyLimits) -> Self {
        self.copy_limits = limits;
        self
    }

    /// Installs an instance-local fault plan for test-support builds.
    #[cfg(feature = "test-support")]
    pub fn test_faults(mut self, plan: crate::TestFaultPlan) -> Self {
        self.test_faults = Some(plan);
        self
    }

    /// Builds the configured filesystem and binds its native authority.
    pub fn build(self) -> LocalResult<LocalFileSystem> {
        validate_limits(self.walk_limits, self.copy_limits)?;
        let filesystem = match self.authority {
            AuthoritySpec::Host => LocalFileSystem::try_host()?.with_symlink_policy(self.symlink_policy)?,
            AuthoritySpec::Rooted(root) => LocalFileSystem::rooted_with_symlink_policy(&root, self.symlink_policy)?,
        };
        let filesystem = filesystem.with_limits(self.walk_limits, self.copy_limits);
        #[cfg(feature = "test-support")]
        let filesystem = filesystem.with_test_faults(self.test_faults);
        Ok(filesystem)
    }
}

fn validate_limits(walk_limits: LocalWalkLimits, copy_limits: LocalCopyLimits) -> LocalResult<()> {
    if [
        walk_limits.max_entries(),
        walk_limits.max_open_handles(),
        copy_limits.max_entries(),
        copy_limits.max_open_handles(),
        copy_limits.max_bytes(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value == 0)
    {
        return Err(
            LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::Configure)
                .with_reason("filesystem resource limits must be positive"),
        );
    }
    Ok(())
}
