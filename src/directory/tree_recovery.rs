// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

#[cfg(test)]
thread_local! {
    static CLEANUP_INDEX_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

use crate::LocalFileError;
use crate::directory::LocalDirectoryHandle;
use crate::observation::LocalFileIdentity;
use crate::options::LocalDeleteOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempCleanupLimits;
use crate::outcome::LocalTreeMutationState;

/// An identity-bound tree resource retained after an operation failure.
///
/// The owner keeps the authority and observed identity needed for explicit
/// cleanup. Dropping an armed owner makes a final best-effort cleanup attempt;
/// call [`Self::retain_in_place`] to relinquish that cleanup responsibility.
/// [`Self::diagnostic_path`] is a namespace coordinate for reporting and does
/// not itself preserve authority or bind later path operations to this entry.
///
/// # Examples
///
/// This helper reports the diagnostic coordinate without consuming the owner;
/// the caller can still retry cleanup afterwards.
///
/// ```
/// use qubit_local_files::directory::LocalTreeRetainedResource;
/// use std::path::Path;
///
/// fn diagnostic_path(resource: &LocalTreeRetainedResource) -> &Path {
///     resource.diagnostic_path()
/// }
/// ```
#[derive(Debug)]
pub struct LocalTreeRetainedResource {
    path: PathBuf,
    relative: PathBuf,
    identity: Option<LocalFileIdentity>,
    parent: LocalDirectoryHandle,
    child: Option<LocalDirectoryHandle>,
    cleanup_armed: bool,
    cleanup_limits: LocalTempCleanupLimits,
    owned_entries: HashMap<PathBuf, LocalFileIdentity>,
    mutation_state: LocalTreeMutationState,
}

impl LocalTreeRetainedResource {
    #[cfg(test)]
    pub(crate) fn reset_cleanup_index_queries() {
        CLEANUP_INDEX_QUERIES.with(|queries| queries.set(0));
    }

    #[cfg(test)]
    pub(crate) fn cleanup_index_queries() -> usize {
        CLEANUP_INDEX_QUERIES.with(std::cell::Cell::get)
    }

    pub(crate) fn new_for_reopen(
        path: PathBuf,
        relative: PathBuf,
        identity: Option<LocalFileIdentity>,
        parent: LocalDirectoryHandle,
        child: Option<LocalDirectoryHandle>,
        cleanup_limits: LocalTempCleanupLimits,
    ) -> Self {
        let mut resource = Self::new(path, relative, identity, parent, child, cleanup_limits);
        resource.cleanup_armed = false;
        resource
    }

    pub(crate) fn arm_after_reopen_validation(&mut self) {
        self.cleanup_armed = true;
    }

    pub(crate) fn new(
        path: PathBuf,
        relative: PathBuf,
        identity: Option<LocalFileIdentity>,
        parent: LocalDirectoryHandle,
        child: Option<LocalDirectoryHandle>,
        cleanup_limits: LocalTempCleanupLimits,
    ) -> Self {
        Self {
            path,
            relative,
            identity,
            parent,
            child,
            cleanup_armed: true,
            cleanup_limits,
            owned_entries: HashMap::new(),
            mutation_state: LocalTreeMutationState::NotStarted,
        }
    }
    /// Returns the namespace path kept for diagnostics.
    ///
    /// This path is not an authority and does not guarantee that resolving it
    /// later reaches the identity-bound resource.
    pub fn diagnostic_path(&self) -> &Path {
        &self.path
    }
    /// Returns the captured native identity, when available.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Sets limits for explicit cleanup and the final Drop attempt.
    ///
    /// Dropping this armed owner runs only a best-effort attempt with these
    /// limits; use [`Self::cleanup`] to observe failures and retain the owner
    /// for retry.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        self.cleanup_limits = limits;
    }
    #[cfg(windows)]
    pub(crate) fn open_child_for_access(&self) -> crate::LocalResult<std::fs::File> {
        let parent = self.parent.root_authority()?.try_clone_authority().map_err(|source| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenRoot,
                Some(self.path.clone()),
                None,
                source,
            )
        })?;
        crate::temp::internal::privacy_windows::open_existing_relative(&parent, &self.relative, true).map_err(
            |source| {
                crate::LocalFileError::from_io(
                    crate::LocalFileOperation::OpenRoot,
                    Some(self.path.clone()),
                    None,
                    source,
                )
            },
        )
    }

    pub(crate) fn disarm_after_publish(&mut self) {
        self.cleanup_armed = false;
        self.child.take();
    }
    pub(crate) fn parent_matches(&self, parent: &LocalDirectoryHandle) -> crate::LocalResult<bool> {
        let expected = self.parent.observe()?.identity().cloned();
        let actual = parent.observe()?.identity().cloned();
        Ok(expected.is_some() && expected == actual)
    }
    pub(crate) fn record_entry(&mut self, relative: &Path, identity: &LocalFileIdentity) -> crate::LocalResult<()> {
        if let Some(recorded_identity) = self.owned_entries.get(relative) {
            if recorded_identity == identity {
                return Ok(());
            }
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::CreateTempFile,
            )
            .with_path(self.path.join(relative)));
        }
        self.owned_entries.try_reserve(1).map_err(|_| {
            crate::LocalFileError::new(
                crate::LocalFileErrorKind::ResourceLimit,
                crate::LocalFileOperation::CreateTempFile,
            )
        })?;
        self.owned_entries.insert(relative.to_path_buf(), identity.clone());
        Ok(())
    }
    fn cleanup_inner(&mut self) -> crate::LocalResult<()> {
        if !self.cleanup_armed {
            return Ok(());
        }
        let started_at = std::time::Instant::now();
        if self.cleanup_limits.max_entries() == Some(0) || cleanup_deadline_exceeded(self.cleanup_limits, started_at) {
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
                #[cfg(test)]
                CLEANUP_INDEX_QUERIES.with(|queries| queries.set(queries.get().saturating_add(1)));
                if self
                    .owned_entries
                    .get(relative.as_path())
                    .is_some_and(|owned_identity| current.identity() != Some(owned_identity))
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
                            .open_directory(&self.relative, &crate::options::LocalDirectoryOpenOptions::new())
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
    /// Removes the retained tree and disarms this owner when cleanup succeeds.
    ///
    /// On failure, returns an error that still owns this resource so cleanup
    /// can be retried or the owner can be transferred elsewhere.
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
    /// Disarms automatic cleanup and returns the recorded namespace path and
    /// identity.
    ///
    /// The returned location is diagnostic metadata, not a cleanup authority.
    /// The consumed owner is disarmed, so this relinquishes its automatic
    /// cleanup responsibility.
    pub fn retain_in_place(self) -> LocalTreeRecoveryLocation {
        let mut this = self;
        this.cleanup_armed = false;
        LocalTreeRecoveryLocation::new(std::mem::take(&mut this.path), this.identity.take())
    }
}

fn cleanup_deadline_exceeded(limits: LocalTempCleanupLimits, started_at: std::time::Instant) -> bool {
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

/// Cleanup failure that retains the identity-bound resource for recovery or
/// retry.
///
/// The cause is borrowed by [`Self::cause`] and exposed through the standard
/// error source chain. The optional retained owner is cleaned best-effort if
/// this error is dropped; extract it to adjust limits and retry explicitly.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeCleanupError;
///
/// fn parts(error: LocalTreeCleanupError) -> (
///     qubit_local_files::LocalFileError,
///     Vec<qubit_local_files::directory::LocalTreeRetainedResource>,
/// ) {
///     error.into_parts()
/// }
/// ```
#[derive(Debug)]
pub struct LocalTreeCleanupError {
    cause: LocalFileError,
    retained: Option<LocalTreeRetainedResource>,
    state: LocalTreeMutationState,
}
impl LocalTreeCleanupError {
    /// Returns the primary cleanup failure without consuming retained owners.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Reports whether cleanup had started mutating the tree before failing.
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.state
    }
    /// Returns cleanup owners that remain available for retry.
    ///
    /// The slice borrows the owners; they remain governed by this error's Drop
    /// behavior until ownership is transferred.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        self.retained.as_slice()
    }
    /// Transfers all retained cleanup owners to the caller.
    ///
    /// The error's primary cause is discarded. Any transferred owner remains
    /// armed and should be explicitly cleaned or deliberately retained.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        self.retained.take().into_iter().collect()
    }
    /// Transfers the cause and all retained cleanup owners.
    ///
    /// This tuple contains every transferable part of this error.
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
impl std::error::Error for LocalTreeCleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// Path and identity recorded after explicitly relinquishing cleanup ownership.
///
/// This value is a diagnostic record only. Its namespace path does not remain
/// bound to the original entry and it carries no authority to access or clean
/// that entry.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeRecoveryLocation;
/// use std::path::Path;
///
/// fn recorded_path(location: &LocalTreeRecoveryLocation) -> &Path {
///     location.namespace_path()
/// }
/// ```
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
    /// Returns the namespace path recorded when cleanup ownership was
    /// relinquished.
    pub fn namespace_path(&self) -> &Path {
        &self.namespace_path
    }
    /// Returns the recorded identity, when one was observed.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
}

/// Failure while creating a staged directory.
///
/// The error owns any stage and retained cleanup resources until they are
/// transferred or the error is dropped. Dropping an unextracted stage or an
/// armed retained owner can trigger its best-effort cleanup.
/// Its cause is borrowed by [`Self::cause`] and exposed as the standard error
/// source.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeCreateError;
/// use qubit_local_files::directory::LocalStagedDirectory;
///
/// fn parts(error: LocalTreeCreateError) -> (
///     qubit_local_files::LocalFileError,
///     Option<LocalStagedDirectory>,
///     Vec<qubit_local_files::directory::LocalTreeRetainedResource>,
/// ) {
///     error.into_parts()
/// }
/// ```
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
    ///
    /// This only borrows them; they remain owned by this error.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers all retained resources to the caller.
    ///
    /// The optional stage is left to be dropped and may run its best-effort
    /// cleanup. Explicitly extract it with [`Self::into_parts`] when needed.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the partially created stage, if its child authority was
    /// established.
    ///
    /// Any retained resources left in this error are dropped and may attempt
    /// cleanup. `None` means no stage owner was established. Use
    /// [`Self::into_parts`] to transfer all owned parts together.
    pub fn into_stage(self) -> Option<super::staged::LocalStagedDirectory> {
        self.stage
    }
    /// Transfers the cause, optional stage, and all retained cleanup owners.
    ///
    /// This is the lossless extraction path for every transferable part.
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
            .or_else(|| self.retained.first().and_then(LocalTreeRetainedResource::identity))
            .or_else(|| self.cause.temp_recovery().and_then(crate::LocalTempRecovery::identity))
    }
}
impl std::fmt::Display for LocalTreeCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "staged directory creation failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreeCreateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// Failure while applying a staged tree access policy.
///
/// The cause is borrowed by [`Self::cause`] and exposed through the standard
/// source chain. Any retained cleanup owners remain owned by this error until
/// extracted; dropping an armed owner can trigger best-effort cleanup. Methods
/// that transfer only owners also discard the cause and access state; use
/// [`Self::into_parts`] when those values are needed.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalTreeAccessError;
///
/// fn parts(error: LocalTreeAccessError) -> (
///     qubit_local_files::LocalFileError,
///     qubit_local_files::outcome::LocalTreeAccessState,
///     Vec<qubit_local_files::directory::LocalTreeRetainedResource>,
/// ) {
///     error.into_parts()
/// }
/// ```
#[derive(Debug)]
pub struct LocalTreeAccessError {
    cause: LocalFileError,
    access_state: crate::outcome::LocalTreeAccessState,
    retained: Vec<LocalTreeRetainedResource>,
}
impl LocalTreeAccessError {
    pub(crate) fn new(cause: LocalFileError, access_state: crate::outcome::LocalTreeAccessState) -> Self {
        Self {
            cause,
            access_state,
            retained: Vec::new(),
        }
    }
    /// Returns the access-policy failure cause.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Returns the access state observed when the operation failed.
    pub const fn access_state(&self) -> crate::outcome::LocalTreeAccessState {
        self.access_state
    }
    /// Borrows cleanup owners that remain held by this error.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers retained cleanup owners and discards the cause and access
    /// state.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the cause, access state, and retained cleanup owners.
    pub fn into_parts(
        mut self,
    ) -> (
        LocalFileError,
        crate::outcome::LocalTreeAccessState,
        Vec<LocalTreeRetainedResource>,
    ) {
        (self.cause, self.access_state, std::mem::take(&mut self.retained))
    }
}
impl std::fmt::Display for LocalTreeAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tree access policy failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreeAccessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// Failure while publishing a staged directory.
///
/// The error owns an unpublished stage and any retained cleanup resources.
/// Inspect publication, mutation, and access state before consuming it, then
/// use [`Self::into_parts`] to take every transferable owner. Dropping an
/// unextracted stage or armed retained owner can trigger best-effort cleanup.
/// The borrowed cause is also exposed as the standard error source.
///
/// # Examples
///
/// The conflict path below inspects the state first, extracts all owners, and
/// explicitly cleans each one. Cleanup errors are returned with their owner
/// and any unprocessed owners still available for retry.
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::LocalTreePublishError;
/// use qubit_local_files::directory::{LocalStagedDirectory, LocalTreeCleanupError, LocalTreeRetainedResource};
/// use qubit_local_files::options::{LocalDirectoryOpenOptions, LocalTempDirectoryOptions, LocalTreePublishOptions};
/// use qubit_local_files::outcome::LocalTreePublication;
/// use std::error::Error;
/// use std::ffi::OsStr;
///
/// #[derive(Debug)]
/// struct RecoveryCleanupFailure {
///     failure: LocalTreeCleanupError,
///     remaining: Vec<LocalTreeRetainedResource>,
/// }
///
/// impl std::fmt::Display for RecoveryCleanupFailure {
///     fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         write!(formatter, "cleanup failed with {} retained resources remaining: {}", self.remaining.len(), self.failure)
///     }
/// }
///
/// impl Error for RecoveryCleanupFailure {
///     fn source(&self) -> Option<&(dyn Error + 'static)> {
///         Some(&self.failure)
///     }
/// }
///
/// fn cleanup_all(
///     stage: Option<LocalStagedDirectory>,
///     retained: Vec<LocalTreeRetainedResource>,
/// ) -> Result<(), RecoveryCleanupFailure> {
///     if let Some(stage) = stage {
///         if let Err(failure) = stage.cleanup() {
///             return Err(RecoveryCleanupFailure { failure, remaining: retained });
///         }
///     }
///     let mut remaining = retained.into_iter();
///     while let Some(resource) = remaining.next() {
///         if let Err(failure) = resource.cleanup() {
///             return Err(RecoveryCleanupFailure {
///                 failure,
///                 remaining: remaining.collect(),
///             });
///         }
///     }
///     Ok(())
/// }
///
/// fn recover(failure: LocalTreePublishError) -> Result<(), RecoveryCleanupFailure> {
///     let _publication = failure.publication();
///     let _mutation = failure.mutation_state();
///     let _access = failure.access_state();
///     let (_, stage, retained) = failure.into_parts();
///     cleanup_all(stage, retained)
/// }
///
/// let fixture = tempfile::tempdir()?;
/// std::fs::create_dir(fixture.path().join("target"))?;
/// let filesystem = LocalFileSystem::host()?;
/// let parent = filesystem.open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())?;
/// let stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
/// let failure = stage.publish_new(&parent, OsStr::new("target"), &LocalTreePublishOptions::new())
///     .expect_err("existing target should conflict");
/// assert_eq!(LocalTreePublication::NotPublished, failure.publication());
/// recover(failure)?;
/// # Ok::<(), Box<dyn Error>>(())
/// ```
#[derive(Debug)]
pub struct LocalTreePublishError {
    cause: LocalFileError,
    stage: Option<super::staged::LocalStagedDirectory>,
    retained: Vec<LocalTreeRetainedResource>,
    mutation: LocalTreeMutationState,
    publication: crate::outcome::LocalTreePublication,
    access_state: crate::outcome::LocalTreeAccessState,
}
impl LocalTreePublishError {
    pub(crate) fn new(
        cause: LocalFileError,
        stage: Option<super::staged::LocalStagedDirectory>,
        mutation: LocalTreeMutationState,
        publication: crate::outcome::LocalTreePublication,
        access_state: crate::outcome::LocalTreeAccessState,
    ) -> Self {
        Self {
            cause,
            stage,
            retained: Vec::new(),
            mutation,
            publication,
            access_state,
        }
    }
    pub(crate) fn with_retained(mut self, retained: Option<LocalTreeRetainedResource>) -> Self {
        if let Some(retained) = retained {
            self.retained.push(retained);
        }
        self
    }
    /// Returns the publication failure without consuming any recovery owner.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Returns whether publication may have mutated the destination namespace.
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.mutation
    }
    /// Borrows retained cleanup owners; they remain governed by this error's
    /// Drop behavior until extracted.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Returns the publication outcome recorded before the failure.
    pub const fn publication(&self) -> crate::outcome::LocalTreePublication {
        self.publication
    }
    /// Returns the access state observed when publication failed.
    pub const fn access_state(&self) -> crate::outcome::LocalTreeAccessState {
        self.access_state
    }
    /// Transfers the optional stage. If it is `None`, no stage owner remains.
    /// Any unextracted retained owners are dropped and may attempt cleanup;
    /// use [`Self::into_parts`] to take all owners.
    pub fn into_stage(self) -> Option<super::staged::LocalStagedDirectory> {
        self.stage
    }
    /// Transfers retained cleanup owners. The optional stage is dropped and
    /// may attempt best-effort cleanup; use [`Self::into_parts`] to transfer
    /// the stage as well.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the cause, optional stage, and retained cleanup owners.
    /// Inspect publication, mutation, and access state before calling this
    /// method because those status values are not part of the returned tuple.
    pub fn into_parts(
        mut self,
    ) -> (
        LocalFileError,
        Option<super::staged::LocalStagedDirectory>,
        Vec<LocalTreeRetainedResource>,
    ) {
        (self.cause, self.stage.take(), std::mem::take(&mut self.retained))
    }
}
impl std::fmt::Display for LocalTreePublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tree publish failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreePublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
