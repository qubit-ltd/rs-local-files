use std::path::{Path, PathBuf};

use crate::LocalFileError;
use crate::directory::LocalDirectoryHandle;
use crate::observation::LocalFileIdentity;
use crate::options::LocalTempCleanupLimits;
use crate::options::{LocalDeleteOptions, LocalListOptions};
use crate::outcome::LocalTreeMutationState;

/// An identity-bound tree resource retained after an operation failure.
#[derive(Debug)]
pub struct LocalTreeRetainedResource {
    path: PathBuf,
    relative: PathBuf,
    identity: Option<LocalFileIdentity>,
    parent: LocalDirectoryHandle,
    child: Option<LocalDirectoryHandle>,
    cleanup_armed: bool,
    cleanup_limits: LocalTempCleanupLimits,
    owned_entries: Vec<(PathBuf, LocalFileIdentity)>,
    mutation_state: LocalTreeMutationState,
}

impl LocalTreeRetainedResource {
    pub(crate) fn new(
        path: PathBuf,
        relative: PathBuf,
        identity: Option<LocalFileIdentity>,
        parent: LocalDirectoryHandle,
        child: Option<LocalDirectoryHandle>,
    ) -> Self {
        Self {
            path,
            relative,
            identity,
            parent,
            child,
            cleanup_armed: true,
            cleanup_limits: LocalTempCleanupLimits::new(),
            owned_entries: Vec::new(),
            mutation_state: LocalTreeMutationState::NotStarted,
        }
    }
    /// Returns the namespace path kept for diagnostics.
    pub fn diagnostic_path(&self) -> &Path {
        &self.path
    }
    /// Returns the captured native identity, when available.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Sets limits for explicit cleanup and the final Drop attempt.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        self.cleanup_limits = limits;
    }
    pub(crate) fn record_entry(
        &mut self,
        relative: &Path,
        identity: &LocalFileIdentity,
    ) -> crate::LocalResult<()> {
        self.owned_entries.try_reserve(1).map_err(|_| {
            crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                crate::LocalFileOperation::CreateTempFile,
            )
        })?;
        self.owned_entries
            .push((relative.to_path_buf(), identity.clone()));
        Ok(())
    }
    fn cleanup_inner(&mut self) -> crate::LocalResult<()> {
        if !self.cleanup_armed {
            return Ok(());
        }
        let started_at = std::time::Instant::now();
        if self.cleanup_limits.max_entries() == Some(0)
            || cleanup_deadline_exceeded(self.cleanup_limits, started_at)
        {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                crate::LocalFileOperation::Cleanup,
            ));
        }
        let named = self.parent.observe_entry(&self.relative)?;
        if self
            .identity
            .as_ref()
            .is_none_or(|identity| named.identity() != Some(identity))
        {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Cleanup,
            )
            .with_path(self.path.clone()));
        }
        if let Some(child) = self.child.as_ref() {
            let mut entries = Vec::new();
            let mut pending_path_bytes = self.path.as_os_str().as_encoded_bytes().len();
            let mut list_options = LocalListOptions::new()
                .with_recursive()
                .with_symlink_policy(crate::LocalSymlinkPolicy::Reject);
            if let Some(maximum) = self.cleanup_limits.max_depth() {
                list_options = list_options.with_max_depth(maximum);
            }
            if let Some(maximum) = self.cleanup_limits.max_entries() {
                list_options = list_options.with_max_entries(maximum.saturating_sub(1));
            }
            if let Some(maximum) = self.cleanup_limits.max_pending_path_bytes() {
                list_options = list_options.with_max_seen_name_bytes(maximum);
            }
            if let Some(deadline) = self.cleanup_limits.deadline() {
                list_options = list_options.with_deadline(deadline);
            }
            for entry in child.list_with_options(Path::new("."), &list_options)? {
                if cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    ));
                }
                let entry = entry?;
                let relative = entry.relative_path().to_path_buf();
                pending_path_bytes = pending_path_bytes
                    .checked_add(relative.as_os_str().as_encoded_bytes().len())
                    .ok_or_else(|| {
                        crate::LocalFileError::new(
                            crate::LocalFileErrorKind::ResourceLimit,
                            crate::LocalFileOperation::Cleanup,
                        )
                    })?;
                if self
                    .cleanup_limits
                    .max_pending_path_bytes()
                    .is_some_and(|maximum| pending_path_bytes > maximum)
                {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    ));
                }
                let observation = child.observe_entry(&relative)?;
                let identity = observation.identity().cloned().ok_or_else(|| {
                    crate::LocalFileError::new(
                        crate::LocalFileErrorKind::Unsupported,
                        crate::LocalFileOperation::Cleanup,
                    )
                    .with_path(self.path.join(&relative))
                })?;
                entries.try_reserve(1).map_err(|_| {
                    crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    )
                })?;
                entries.push((relative, observation.metadata().kind(), identity));
            }
            entries.sort_by_key(|(path, _, _)| std::cmp::Reverse(path.components().count()));
            for (relative, kind, expected) in entries {
                if cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::Cleanup,
                    ));
                }
                let current = child.observe_entry(&relative)?;
                if current.identity() != Some(&expected) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::InvalidState,
                        crate::LocalFileOperation::Cleanup,
                    )
                    .with_path(self.path.join(&relative)));
                }
                if self
                    .owned_entries
                    .iter()
                    .any(|(owned_path, owned_identity)| {
                        owned_path == &relative && current.identity() != Some(owned_identity)
                    })
                {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::InvalidState,
                        crate::LocalFileOperation::Cleanup,
                    )
                    .with_path(self.path.join(&relative)));
                }
                let result = if kind == crate::LocalFileKind::Directory {
                    child.filesystem().delete_directory(&relative).map(|_| ())
                } else {
                    child.filesystem().delete_file(&relative).map(|_| ())
                };
                self.mutation_state = LocalTreeMutationState::Started;
                result?;
            }
            drop(self.child.take());
        }
        match self.parent.observe_entry(&self.relative) {
            Ok(observation)
                if self
                    .identity
                    .as_ref()
                    .is_some_and(|identity| observation.identity() == Some(identity)) =>
            {
                self.mutation_state = LocalTreeMutationState::Started;
                match self
                    .parent
                    .filesystem()
                    .delete_directory_with_options(&self.relative, &LocalDeleteOptions::new())
                {
                    Ok(_) => Ok(()),
                    Err(error) => {
                        self.child = self
                            .parent
                            .open_directory(
                                &self.relative,
                                &crate::options::LocalDirectoryOpenOptions::new(),
                            )
                            .ok();
                        Err(error)
                    }
                }
            }
            Ok(_) => Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Cleanup,
            )
            .with_path(self.path.clone())),
            Err(cause) => Err(cause),
        }
    }
    pub fn cleanup(mut self) -> Result<(), LocalTreeCleanupError> {
        match self.cleanup_inner() {
            Ok(()) => {
                self.cleanup_armed = false;
                Ok(())
            }
            Err(cause) => Err(LocalTreeCleanupError {
                cause,
                state: self.mutation_state,
                retained: Some(self),
            }),
        }
    }
    pub fn retain_in_place(self) -> LocalTreeRecoveryLocation {
        let mut this = self;
        this.cleanup_armed = false;
        LocalTreeRecoveryLocation::new(std::mem::take(&mut this.path), this.identity.take())
    }
}

fn cleanup_deadline_exceeded(
    limits: LocalTempCleanupLimits,
    started_at: std::time::Instant,
) -> bool {
    limits
        .deadline()
        .is_some_and(|deadline| started_at.elapsed() >= deadline)
}

impl Drop for LocalTreeRetainedResource {
    fn drop(&mut self) {
        if self.cleanup_armed {
            let _ = self.cleanup_inner();
        }
    }
}

/// Cleanup failure that retains the identity-bound resource for recovery or retry.
#[derive(Debug)]
pub struct LocalTreeCleanupError {
    cause: LocalFileError,
    retained: Option<LocalTreeRetainedResource>,
    state: LocalTreeMutationState,
}
impl LocalTreeCleanupError {
    /// Returns the primary cleanup failure.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Reports whether cleanup had started mutating the tree before failing.
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.state
    }
    /// Returns cleanup owners that remain available for retry.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        self.retained.as_slice()
    }
    /// Transfers all retained cleanup owners to the caller.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        self.retained.take().into_iter().collect()
    }
    /// Transfers the cause and all retained cleanup owners.
    pub fn into_parts(mut self) -> (LocalFileError, Vec<LocalTreeRetainedResource>) {
        let cause = std::mem::replace(
            &mut self.cause,
            LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Cleanup,
            ),
        );
        (cause, self.retained.take().into_iter().collect())
    }
}
impl std::fmt::Display for LocalTreeCleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tree cleanup failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreeCleanupError {}

/// Path and identity recorded after explicitly relinquishing cleanup ownership.
#[derive(Clone, Debug)]
pub struct LocalTreeRecoveryLocation {
    namespace_path: PathBuf,
    identity: Option<LocalFileIdentity>,
}
impl LocalTreeRecoveryLocation {
    pub(crate) fn new(namespace_path: PathBuf, identity: Option<LocalFileIdentity>) -> Self {
        Self {
            namespace_path,
            identity,
        }
    }
    /// Returns the namespace path recorded when cleanup ownership was relinquished.
    pub fn namespace_path(&self) -> &Path {
        &self.namespace_path
    }
    /// Returns the recorded identity, when one was observed.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
}

/// Failure while creating a staged directory.
#[derive(Debug)]
pub struct LocalTreeCreateError {
    cause: LocalFileError,
    stage: Option<super::staged::LocalStagedDirectory>,
    retained: Vec<LocalTreeRetainedResource>,
    state: LocalTreeMutationState,
}
impl LocalTreeCreateError {
    pub(crate) fn new(
        cause: LocalFileError,
        stage: Option<super::staged::LocalStagedDirectory>,
        retained: Vec<LocalTreeRetainedResource>,
        state: LocalTreeMutationState,
    ) -> Self {
        Self {
            cause,
            stage,
            retained,
            state,
        }
    }
    /// Returns the primary stage creation failure.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Reports whether stage creation changed the namespace before failing.
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.state
    }
    /// Returns owners for any partially created stage resources.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers all retained resources to the caller.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the partially created stage, if its child authority was established.
    pub fn into_stage(self) -> Option<super::staged::LocalStagedDirectory> {
        self.stage
    }
    /// Transfers the cause, optional stage, and all retained cleanup owners.
    pub fn into_parts(
        self,
    ) -> (
        LocalFileError,
        Option<super::staged::LocalStagedDirectory>,
        Vec<LocalTreeRetainedResource>,
    ) {
        (self.cause, self.stage, self.retained)
    }
    /// Returns the name of a fully or partially created stage, when known.
    pub fn stage_name(&self) -> Option<&std::ffi::OsStr> {
        self.stage
            .as_ref()
            .map(super::staged::LocalStagedDirectory::relative_name)
            .or_else(|| {
                self.retained
                    .first()
                    .and_then(|resource| resource.diagnostic_path().file_name())
            })
            .or_else(|| self.cause.path().and_then(Path::file_name))
    }
    /// Returns the identity of a fully or partially created stage, when known.
    pub fn stage_identity(&self) -> Option<&LocalFileIdentity> {
        self.stage
            .as_ref()
            .and_then(super::staged::LocalStagedDirectory::identity)
            .or_else(|| {
                self.retained
                    .first()
                    .and_then(LocalTreeRetainedResource::identity)
            })
            .or_else(|| {
                self.cause
                    .temp_recovery()
                    .and_then(crate::LocalTempRecovery::identity)
            })
    }
}
impl std::fmt::Display for LocalTreeCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "staged directory creation failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreeCreateError {}
