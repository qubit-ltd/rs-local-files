// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup-owned temporary files with host or rooted authority.

use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::IoSlice;
use std::io::Result;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::internal::LocalTempResourceBackend;
use super::internal::LocalTempResourceCore;
use super::internal::LocalTempResourceState;
use super::internal::RootedTempResourceBackend;
use super::internal::TempEntryIdentity;
use super::internal::generated_target;
use super::internal::prepare_host_parent;
use super::internal::prepare_rooted_parent;
use crate::LocalDurabilityRequirement;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalPersistError;
use crate::LocalPersistFailureState;
use crate::LocalPersistMethod;
use crate::LocalPersistOptions;
use crate::LocalPersistOutcome;
use crate::LocalPersistStage;
use crate::LocalRelativePath;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::outcome::LocalTempSourceState;
use crate::path::LocalFileSystemScope;
use crate::path::LocalPathResolver;

/// A temporary file whose cleanup remains bound to its creating authority.
///
/// Cleanup rejects ordinary path replacement by checking the identity captured
/// at creation. The check and deletion are not atomic, so callers must exclude
/// untrusted concurrent mutation of the containing directory; identity reuse
/// and a check/delete race cannot be ruled out by this path-based API.
/// Persistence resolves intermediate symbolic links using the policy captured
/// by the creating [`crate::LocalFileSystem`], while replacing a final link
/// entry itself.
///
/// The file is created inside a private generated sandbox. Cleanup removes the
/// file and then the empty sandbox. [`Self::keep`] atomically publishes the
/// file outside that sandbox.
///
/// # Examples
///
/// ```no_run
/// use std::io::Write;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut temporary = filesystem.create_temp_file()?;
/// temporary.write_all(b"temporary data")?;
/// temporary.cleanup()?;
/// # Ok(())
/// # }
/// ```
#[must_use = "use explicit cleanup to observe errors; drop only attempts cleanup"]
#[derive(Debug)]
pub struct LocalTempFile {
    /// Bound authority and shared source lifecycle.
    core: LocalTempResourceCore,
    /// Open file handle until closed or source authority is invalidated.
    file: Option<File>,
}

impl LocalTempFile {
    /// Builds a host temporary file from its already-bound path and handle.
    /// Takes ownership of `file` and captures its identity. Native inspection
    /// failures close the handle; the caller remains responsible for removing
    /// the already-created file and sandbox when construction fails.
    #[inline]
    pub(crate) fn host(
        path: PathBuf,
        sandbox_path: PathBuf,
        file: File,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        let identity = TempEntryIdentity::from_file(&file)?;
        Ok(Self {
            core: LocalTempResourceCore::new(
                path,
                LocalTempResourceBackend::Host(super::internal::HostTempResourceBackend { sandbox_path, identity }),
                symlink_policy,
            ),
            file: Some(file),
        })
    }

    /// Builds a rooted temporary file from the retained root authority.
    /// Takes ownership of `file` and captures identity through that handle.
    /// On native inspection failure, the caller must clean the already-created
    /// resource and sandbox through `root`; the file handle is dropped.
    #[inline]
    pub(crate) fn rooted(
        root: Arc<crate::rooted::Root>,
        path: PathBuf,
        sandbox_path: PathBuf,
        file: File,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        let identity = crate::rooted::Metadata::from_open_file(&file)?;
        Ok(Self {
            core: LocalTempResourceCore::new(
                path.clone(),
                LocalTempResourceBackend::Rooted(RootedTempResourceBackend {
                    root,
                    relative_path: path,
                    sandbox_path,
                    identity,
                }),
                symlink_policy,
            ),
            file: Some(file),
        })
    }

    /// Returns the namespace-absolute generated path.
    #[must_use]
    #[inline]
    pub fn path(&self) -> &Path {
        &self.core.path
    }

    /// Returns the current source authority, independently of earlier
    /// publication.
    #[must_use = "inspect the source authority before choosing a recovery action"]
    #[inline]
    pub const fn source_state(&self) -> LocalTempSourceState {
        self.core.source_state()
    }

    /// Closes the file I/O handle while retaining cleanup and persistence
    /// responsibility.
    #[inline]
    pub fn close(&mut self) {
        drop(self.file.take());
    }

    /// Removes the entry through the authority retained at creation time.
    ///
    /// Closes the I/O handle first. Returns a structured error when identity
    /// inspection, entry removal, or empty-sandbox removal fails, or namespace
    /// certainty makes cleanup unsafe. A sandbox-only failure can be retried;
    /// success is idempotent. This does not undo an already-published file.
    pub fn cleanup(&mut self) -> LocalResult<()> {
        self.close();
        self.ensure_cleanup_safe().map_err(|error| {
            self.contextualize_error(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.core.path.clone()),
                None,
                error,
            ))
        })?;
        if self.core.state == LocalTempResourceState::Owned {
            self.remove_resource().map_err(|error| {
                self.contextualize_error(LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.core.path.clone()),
                    None,
                    error,
                ))
            })?;
            self.core.state = LocalTempResourceState::SandboxPending;
        }
        if self.core.state == LocalTempResourceState::SandboxPending {
            self.release_sandbox().map_err(|error| {
                self.contextualize_error(LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.cleanup_path()),
                    None,
                    error,
                ))
            })?;
            self.core.state = LocalTempResourceState::Released;
        }
        Ok(())
    }

    /// Atomically publishes the file to a generated sibling outside its
    /// private sandbox.
    /// Uses the default no-replacement policy of [`Self::persist_with`], with
    /// the same publication outcome and resource-retaining failure contract.
    #[inline]
    pub fn keep(self) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        let requested_target = self.core.path.clone();
        if let Err(error) = self.core.ensure_publishable() {
            return Err(self.persist_error(
                error,
                requested_target,
                None,
                LocalPersistStage::InstallDestination,
                LocalPersistFailureState::NotPublished,
            ));
        }
        let target = match generated_target(&requested_target) {
            Ok(target) => target,
            Err(error) => {
                return Err(self.persist_error(
                    error,
                    requested_target,
                    None,
                    LocalPersistStage::ResolveTarget,
                    LocalPersistFailureState::NotPublished,
                ));
            }
        };
        self.persist_with_path(&target, None, LocalPersistOptions::new())
    }

    /// Persists the file within its creating authority without replacement.
    /// Uses [`Self::persist_with`] with default options and the same errors.
    #[inline]
    pub fn persist(
        self,
        target: impl AsRef<Path>,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with(target, LocalPersistOptions::new())
    }

    /// Persists the file with explicit replacement policy within its creating
    /// authority.
    ///
    /// Consumes this guard and requires a nonempty namespace-absolute target.
    /// Use [`Self::persist_at`] for relative targets with an explicit base.
    /// Invalid targets retain the original open guard before synchronization.
    /// `options` selects replacement, parent creation, and durability.
    /// Returns the achieved publication guarantees and any sandbox cleanup
    /// error after a successful install.
    ///
    /// Identity, policy, resolution, parent creation, installation, and
    /// synchronization failures return a `LocalPersistError` retaining this
    /// resource, its failure stage, and publication certainty. The retained
    /// file may already be closed; created parents and a published destination
    /// are not rolled back. Inspect the error state before retry or cleanup.
    #[inline]
    pub fn persist_with(
        self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path(target.as_ref(), None, options)
    }

    /// Returns the mutable open file handle, or an error after [`Self::close`]
    /// or when source authority no longer permits file operations.
    #[inline]
    pub fn as_file_mut(&mut self) -> Result<&mut File> {
        self.core.ensure_publishable()?;
        self.file.as_mut().ok_or_else(closed_file_error)
    }

    /// Persists a relative target against an explicit namespace-absolute base.
    ///
    /// `base` must be an existing directory without dot or parent components;
    /// `target` must be nonempty and strictly relative. Both are interpreted
    /// through the creating authority and its captured symlink policy. This
    /// never reads the process PWD. Invalid parameters return `ResolveTarget`
    /// before source synchronization or closing. Other stages and publication
    /// guarantees match [`Self::persist_with`]. Errors retain the resource.
    pub fn persist_at(
        self,
        base: &Path,
        target: &Path,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path(target, Some(base), options)
            .map_err(|error| error.with_current_directory(base.to_path_buf()))
    }

    /// Binds public paths and records the creating filesystem PWD for
    /// diagnostics. Returns a contextual path-resolution error on invalid
    /// namespace input; consuming failure drops this guard and attempts
    /// cleanup.
    pub(crate) fn bind_namespace(mut self, resolver: LocalPathResolver) -> LocalResult<Self> {
        self.core.bind_namespace(resolver, LocalFileOperation::CreateTempFile)?;
        Ok(self)
    }

    /// Persists the file to a resolved public-API target path.
    /// Implements the ownership, guarantee, and failure contract of
    /// [`Self::persist_with`], using only the retained authority and explicit
    /// base.
    fn persist_with_path(
        mut self,
        target: &Path,
        base: Option<&Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        let requested_target = target.to_path_buf();
        if let Err(error) = self.core.ensure_publishable() {
            return Err(self.persist_error(
                error,
                requested_target,
                None,
                LocalPersistStage::InstallDestination,
                LocalPersistFailureState::NotPublished,
            ));
        }
        let scope = self.core.scope();
        let resolved_target = match self.core.prepare_target(base, target) {
            Ok(target) => target,
            Err(error) => {
                return Err(self.persist_error(
                    error.into_io_error(),
                    requested_target,
                    None,
                    LocalPersistStage::ResolveTarget,
                    LocalPersistFailureState::NotPublished,
                ));
            }
        };
        let namespace_target = resolved_target.namespace_absolute().to_path_buf();
        if scope == LocalFileSystemScope::Rooted && resolved_target.authority_relative().as_os_str().is_empty() {
            return Err(self.persist_error(
                Error::new(ErrorKind::InvalidInput, "cannot replace the Rooted virtual root"),
                requested_target,
                Some(namespace_target),
                LocalPersistStage::ResolveTarget,
                LocalPersistFailureState::NotPublished,
            ));
        }
        if resolved_target.directory_required() {
            return Err(self.persist_error(
                Error::from(ErrorKind::NotADirectory),
                requested_target,
                Some(namespace_target),
                LocalPersistStage::ResolveTarget,
                LocalPersistFailureState::NotPublished,
            ));
        }
        if let Some(base) = base
            && let Err(error) = self.core.validate_base(base)
        {
            return Err(self.persist_error(
                error.into_io_error(),
                requested_target,
                Some(namespace_target),
                LocalPersistStage::ResolveTarget,
                LocalPersistFailureState::NotPublished,
            ));
        }
        if let Err(error) = self.ensure_identity_matches() {
            return Err(self.persist_error(
                error,
                requested_target,
                Some(namespace_target),
                LocalPersistStage::InstallDestination,
                LocalPersistFailureState::NotPublished,
            ));
        }

        let file_durable = match self.synchronize_source(options.durability()) {
            Ok(durable) => durable,
            Err(error) => {
                return Err(self.persist_error(
                    error,
                    requested_target,
                    Some(namespace_target),
                    LocalPersistStage::SynchronizeSource,
                    LocalPersistFailureState::NotPublished,
                ));
            }
        };
        self.close();
        let authority_target = resolved_target.authority_relative().to_path_buf();
        if matches!(&self.core.backend, LocalTempResourceBackend::Host(_)) {
            let target = match crate::local::resolve_host_path(&authority_target, self.core.symlink_policy, false) {
                Ok(target) => target,
                Err(error) => {
                    return Err(self.persist_error(
                        error.into_io_error(),
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::ResolveTarget,
                        LocalPersistFailureState::NotPublished,
                    ));
                }
            };
            let parent_dirs_to_sync = match prepare_host_parent(&target, options.creates_parent()) {
                Ok(parent_dirs) => parent_dirs,
                Err(error) => {
                    return Err(self.persist_error(
                        error,
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::PrepareParent,
                        LocalPersistFailureState::NotPublished,
                    ));
                }
            };
            let result = if options.overwrites() {
                crate::local::replace_file(&self.core.path, &target)
            } else {
                crate::local::move_file_without_replacing(&self.core.path, &target)
            };
            if let Err(error) = result {
                let publication = self.record_native_persist_failure(&error);
                return Err(self.persist_error(
                    error,
                    requested_target,
                    Some(namespace_target),
                    LocalPersistStage::InstallDestination,
                    publication,
                ));
            }
            self.core.state = LocalTempResourceState::SandboxPending;
            let parent_durable = match synchronize_host_publication(
                &self.core.path,
                &target,
                &parent_dirs_to_sync,
                options.durability(),
            ) {
                Ok(durable) => durable,
                Err(error) => {
                    return Err(self.persist_error(
                        error,
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::SynchronizeDestination,
                        LocalPersistFailureState::Published,
                    ));
                }
            };
            let cleanup_error = self.release_sandbox().err().map(|error| {
                self.contextualize_error(LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.cleanup_path()),
                    None,
                    error,
                ))
            });
            self.core.state = LocalTempResourceState::Released;
            return Ok(LocalPersistOutcome::new(
                namespace_target,
                LocalPersistMethod::AtomicRename,
                true,
                file_durable && parent_durable,
                cleanup_error,
            ));
        }
        let target = match LocalRelativePath::new(&authority_target) {
            Ok(path) => path.as_path().to_path_buf(),
            Err(error) => {
                return Err(self.persist_error(
                    error,
                    requested_target,
                    Some(namespace_target),
                    LocalPersistStage::ResolveTarget,
                    LocalPersistFailureState::NotPublished,
                ));
            }
        };
        let LocalTempResourceBackend::Rooted(rooted) = &self.core.backend else {
            unreachable!()
        };
        let source =
            LocalRelativePath::new(&rooted.relative_path).expect("rooted temporary path was validated at creation");
        let resolved = match crate::rooted_local_file_system::resolve_rooted_path(
            &rooted.root,
            &target,
            self.core.symlink_policy,
            false,
            LocalFileOperation::PersistTemp,
        ) {
            Ok(resolved) => resolved,
            Err(error) => {
                return Err(self.persist_error(
                    error.into_io_error(),
                    requested_target,
                    Some(namespace_target),
                    LocalPersistStage::ResolveTarget,
                    LocalPersistFailureState::NotPublished,
                ));
            }
        };
        let destination = resolved;
        if let Err(error) = prepare_rooted_parent(&rooted.root, &destination, options.creates_parent()) {
            return Err(self.persist_error(
                error,
                requested_target,
                Some(namespace_target),
                LocalPersistStage::PrepareParent,
                LocalPersistFailureState::NotPublished,
            ));
        }
        let result = if options.overwrites() {
            rooted.root.rename(&source, &destination)
        } else {
            rooted.root.rename_without_replacing(&source, &destination)
        };
        if let Err(error) = result {
            let publication = self.record_native_persist_failure(&error);
            return Err(self.persist_error(
                error,
                requested_target,
                Some(namespace_target),
                LocalPersistStage::InstallDestination,
                publication,
            ));
        }
        self.core.state = LocalTempResourceState::SandboxPending;
        let parent_durable =
            match synchronize_rooted_publication(&rooted.root, &source, &destination, options.durability()) {
                Ok(durable) => durable,
                Err(error) => {
                    return Err(self.persist_error(
                        error,
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::SynchronizeDestination,
                        LocalPersistFailureState::Published,
                    ));
                }
            };
        let cleanup_error = self.release_sandbox().err().map(|error| {
            self.contextualize_error(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.cleanup_path()),
                None,
                error,
            ))
        });
        self.core.state = LocalTempResourceState::Released;
        Ok(LocalPersistOutcome::new(
            namespace_target,
            LocalPersistMethod::AtomicRename,
            true,
            file_durable && parent_durable,
            cleanup_error,
        ))
    }

    /// Synchronizes temporary file contents before namespace publication.
    /// Returns false without I/O for `NotRequired`, or when a preferred sync
    /// cannot be achieved. Required mode propagates unsupported-platform and
    /// native open/sync errors. A closed file is reopened through its backend.
    fn synchronize_source(&self, durability: LocalDurabilityRequirement) -> Result<bool> {
        if durability == LocalDurabilityRequirement::NotRequired {
            return Ok(false);
        }
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("temp-file-source-sync") {
            return Err(crate::local::test_fault_error());
        }
        let durability_supported = match &self.core.backend {
            LocalTempResourceBackend::Host(_) => {
                crate::LocalFileSystemCapabilities::detect_host().supports_durable_temp_file_persist()
            }
            LocalTempResourceBackend::Rooted(_) => {
                crate::LocalFileSystemCapabilities::detect_rooted().supports_durable_temp_file_persist()
            }
        };
        if !durability_supported {
            return match durability {
                LocalDurabilityRequirement::Required => Err(Error::new(
                    ErrorKind::Unsupported,
                    "required temporary-file persistence durability is unavailable on this platform",
                )),
                LocalDurabilityRequirement::Preferred | LocalDurabilityRequirement::NotRequired => Ok(false),
            };
        }
        let synchronize = || -> Result<()> {
            if let Some(file) = self.file.as_ref() {
                return file.sync_all();
            }
            match &self.core.backend {
                LocalTempResourceBackend::Host(_) => File::open(&self.core.path)?.sync_all(),
                LocalTempResourceBackend::Rooted(rooted) => {
                    let path = LocalRelativePath::new(&rooted.relative_path)?;
                    rooted.root.open_probe_file(&path)?.sync_all()
                }
            }
        };
        match durability {
            LocalDurabilityRequirement::Required => synchronize().map(|()| true),
            LocalDurabilityRequirement::Preferred => Ok(synchronize().is_ok()),
            LocalDurabilityRequirement::NotRequired => Ok(false),
        }
    }

    /// Removes the resource using the retained backend rather than a diagnostic
    /// path.
    /// Rechecks identity before removal and propagates inspection/removal
    /// errors. An identity mismatch marks namespace state indeterminate.
    #[inline]
    fn remove_resource(&mut self) -> Result<()> {
        self.ensure_identity_matches()?;
        match &self.core.backend {
            LocalTempResourceBackend::Host(_) => {
                std::fs::remove_file(&self.core.path)?;
                Ok(())
            }
            LocalTempResourceBackend::Rooted(rooted) => {
                let path = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                rooted.root.remove_file(&path)?;
                Ok(())
            }
        }
    }

    /// Removes the now-empty private sandbox.
    /// Propagates native removal errors, including a non-empty sandbox.
    fn release_sandbox(&self) -> Result<()> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("temp-file-sandbox-remove") {
            return Err(crate::local::test_fault_error());
        }
        self.core.release_sandbox()
    }

    /// Returns the authority-local sandbox path used for cleanup diagnostics.
    fn cleanup_path(&self) -> PathBuf {
        self.core.cleanup_path()
    }

    /// Builds a persistence failure with the resource's creation-time PWD.
    fn persist_error(
        self,
        error: Error,
        requested_target: PathBuf,
        resolved_target: Option<PathBuf>,
        stage: LocalPersistStage,
        publication: LocalPersistFailureState,
    ) -> LocalPersistError<Self> {
        let requirement_not_met =
            stage == LocalPersistStage::SynchronizeSource && error.kind() == ErrorKind::Unsupported;
        let error = self
            .core
            .persist_error(error, requested_target, resolved_target, stage, publication);
        let error = if requirement_not_met {
            error.with_kind(LocalFileErrorKind::RequirementNotMet)
        } else {
            error
        };
        error.with_resource(self)
    }

    /// Attaches the resource's creation-time PWD to a structured error.
    fn contextualize_error(&self, error: LocalFileError) -> LocalFileError {
        self.core.contextualize_error(error)
    }

    /// Rejects namespace cleanup after an indeterminate native publication
    /// attempt.
    #[inline]
    fn ensure_cleanup_safe(&self) -> Result<()> {
        self.core.ensure_cleanup_safe()
    }

    /// Rejects operations when the authority path no longer names this file.
    /// A mismatch marks the state indeterminate and returns `InvalidInput`;
    /// failed identity inspection also marks the state indeterminate while
    /// preserving its native error.
    fn ensure_identity_matches(&mut self) -> Result<()> {
        let result = self.core.ensure_identity_matches();
        if self.core.state == LocalTempResourceState::Indeterminate {
            self.close();
        }
        result
    }

    /// Records whether a failed native install proves the source remains owned.
    #[inline]
    fn record_native_persist_failure(&mut self, error: &Error) -> LocalPersistFailureState {
        self.core.record_native_persist_failure(error)
    }
}

/// Synchronizes both Host rename parents and every created destination parent.
fn synchronize_host_publication(
    source: &Path,
    target: &Path,
    created_parents: &[PathBuf],
    durability: LocalDurabilityRequirement,
) -> Result<bool> {
    synchronize_destination(durability, || {
        crate::local::sync_parent_dir(source)?;
        crate::local::sync_parent_dir(target)?;
        for directory in created_parents.iter().rev() {
            crate::local::sync_parent_dir(directory)?;
        }
        Ok(())
    })
}

/// Synchronizes both Rooted rename parents and the destination ancestor chain.
fn synchronize_rooted_publication(
    root: &crate::rooted::Root,
    source: &LocalRelativePath,
    target: &LocalRelativePath,
    durability: LocalDurabilityRequirement,
) -> Result<bool> {
    synchronize_destination(durability, || {
        root.sync_parent(source)?;
        let mut current = target.as_path();
        loop {
            let relative = LocalRelativePath::new(current)?;
            root.sync_parent(&relative)?;
            let Some(parent) = current.parent() else {
                break;
            };
            if parent.as_os_str().is_empty() {
                break;
            }
            current = parent;
        }
        Ok(())
    })
}

/// Applies preferred or required policy to one destination synchronization.
/// Returns false without calling `synchronize` for `NotRequired`; preferred
/// mode reports native failure as false, while required mode propagates it.
fn synchronize_destination(
    durability: LocalDurabilityRequirement,
    synchronize: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    #[cfg(feature = "test-support")]
    if durability != LocalDurabilityRequirement::NotRequired && crate::local::take_test_support("temp-file-parent-sync")
    {
        return Err(crate::local::test_fault_error());
    }
    match durability {
        LocalDurabilityRequirement::NotRequired => Ok(false),
        LocalDurabilityRequirement::Preferred => Ok(synchronize().is_ok()),
        LocalDurabilityRequirement::Required => synchronize().map(|()| true),
    }
}

impl Write for LocalTempFile {
    /// Writes bytes to the still-open temporary file.
    #[inline]
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.as_file_mut()?.write(buffer)
    }

    /// Writes vectored bytes to the still-open temporary file.
    #[inline]
    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> Result<usize> {
        self.as_file_mut()?.write_vectored(buffers)
    }

    /// Flushes the still-open temporary file.
    #[inline]
    fn flush(&mut self) -> Result<()> {
        self.as_file_mut()?.flush()
    }
}

impl Seek for LocalTempFile {
    /// Seeks the still-open temporary file.
    #[inline]
    fn seek(&mut self, position: SeekFrom) -> Result<u64> {
        self.as_file_mut()?.seek(position)
    }
}

impl Drop for LocalTempFile {
    /// Attempts remaining file or sandbox cleanup and discards cleanup errors.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Builds the error used after a temporary file handle was closed.
#[must_use]
#[inline]
fn closed_file_error() -> Error {
    Error::new(ErrorKind::BrokenPipe, "temporary file handle is closed")
}
