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
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use qubit_io::Output;

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
use crate::error::LocalTempSealError;
use crate::options::LocalSealOptions;
use crate::outcome::LocalTempAccessState;
use crate::outcome::LocalTempSourceState;
use crate::path::LocalFileSystemScope;
use crate::path::LocalPathResolver;
use crate::policy::LocalFileSyncRequirement;

#[cfg(all(test, unix))]
thread_local! {
    static FAIL_NEXT_TEMP_SOURCE_IDENTITY_QUERY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAIL_NEXT_TEMP_SOURCE_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(all(test, unix))]
fn inject_temp_source_identity_query_error() {
    FAIL_NEXT_TEMP_SOURCE_IDENTITY_QUERY.with(|selected| selected.set(true));
}

#[cfg(all(test, unix))]
fn inject_temp_source_sync_error() {
    FAIL_NEXT_TEMP_SOURCE_SYNC.with(|selected| selected.set(true));
}

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
/// The file is created inside its own generated per-resource sandbox directory.
/// This directory structure separates the resource's cleanup lifecycle; it
/// does not by itself guarantee access control. The Unix creation helper
/// requests mode `0700`. On Windows, `PlatformDefault` inherits the parent
/// directory ACL. To require privacy, create the file with
/// [`crate::options::LocalTempFileOptions::with_privacy`] set to
/// [`crate::policy::LocalPrivacyRequirement::Required`]; creation returns an
/// error if the requirement cannot be satisfied. Tightening permissions after
/// creation does not remove any exposure window that occurred during creation.
///
/// Cleanup removes the file and then the empty sandbox. [`Self::keep`]
/// atomically publishes the file outside that sandbox. Identity checking and
/// deletion are not atomic, so cleanup has the concurrent-mutation limitation
/// described above.
///
/// # Examples
///
/// ```no_run
/// use qubit_io::Output;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut temporary = filesystem.create_temp_file()?;
/// temporary.write_fully(b"temporary data")?;
/// temporary.cleanup()?;
/// # Ok(())
/// # }
/// ```
///
/// To require privacy during creation, pass explicit options. Creation returns
/// an error if the platform cannot satisfy the requirement:
///
/// ```no_run
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalTempFileOptions;
/// use qubit_local_files::policy::LocalPrivacyRequirement;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let options = LocalTempFileOptions::new()
///     .with_privacy(LocalPrivacyRequirement::Required);
/// let mut temporary = filesystem.create_temp_file_with_options(&options)?;
/// temporary.cleanup()?;
/// # Ok(())
/// # }
/// ```
///
/// `LocalTempFile` implements [`Output`], not `std::io::Write`:
///
/// ```compile_fail
/// fn require_write<T: std::io::Write>() {}
/// require_write::<qubit_local_files::LocalTempFile>();
/// ```
#[must_use = "use explicit cleanup to observe errors; drop only attempts cleanup"]
pub struct LocalTempFile {
    /// Bound authority and shared source lifecycle.
    core: LocalTempResourceCore,
    /// Open file handle until closed or source authority is invalidated.
    file: Option<File>,
    /// Access mode controlling writes after sealing begins.
    access_state: LocalTempAccessState,
    /// Identity observed on the original open handle, when available.
    file_identity: Option<crate::observation::LocalFileIdentity>,
}

impl std::fmt::Debug for LocalTempFile {
    /// Formats only namespace-safe state without exposing the native path.
    ///
    /// # Parameters
    ///
    /// `f` is the formatter that receives the redacted state representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTempFile")
            .field("path", &crate::local_file_system::redacted_path(self.path()))
            .field("source_state", &self.source_state())
            .field("access_state", &self.access_state)
            .field("handle_open", &self.file.is_some())
            .finish()
    }
}

impl LocalTempFile {
    /// Builds a host temporary file from its already-bound path and handle.
    /// Takes ownership of `file` and captures its identity. Native inspection
    /// failures close the handle; the caller remains responsible for removing
    /// the already-created file and sandbox when construction fails.
    ///
    /// # Parameters
    ///
    /// `path` is the bound file path; `sandbox_path` identifies its cleanup
    /// directory; `file` is the newly created open handle; `symlink_policy`
    /// governs later target resolution.
    ///
    /// # Returns
    ///
    /// A guard bound to the supplied host path, sandbox, and open handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the new handle's native identity cannot be
    /// inspected.
    #[inline]
    pub(crate) fn host(
        path: PathBuf,
        sandbox_path: PathBuf,
        file: File,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        let identity = TempEntryIdentity::from_file(&file)?;
        let file_identity = crate::observation::query::query_observation(
            &file,
            crate::observation::LocalObservationRequirement::Optional,
        )
        .ok()
        .and_then(|observation| observation.identity().cloned());
        let creation_parent = sandbox_path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        Ok(Self {
            core: LocalTempResourceCore::new(
                path,
                creation_parent,
                LocalTempResourceBackend::Host(super::internal::HostTempResourceBackend { sandbox_path, identity }),
                symlink_policy,
            ),
            file: Some(file),
            access_state: LocalTempAccessState::Writable,
            file_identity,
        })
    }

    /// Builds a rooted temporary file from the retained root authority.
    /// Takes ownership of `file` and captures identity through that handle.
    /// On native inspection failure, the caller must clean the already-created
    /// resource and sandbox through `root`; the file handle is dropped.
    ///
    /// # Parameters
    ///
    /// `root` retains rooted authority; `path` is the rooted file path;
    /// `sandbox_path` identifies its cleanup directory; `file` is the new open
    /// handle; `symlink_policy` governs later target resolution.
    ///
    /// # Returns
    ///
    /// A guard bound to the supplied root authority, path, sandbox, and handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the new handle's rooted identity cannot be
    /// inspected.
    #[inline]
    pub(crate) fn rooted(
        root: Arc<crate::rooted::Root>,
        path: PathBuf,
        sandbox_path: PathBuf,
        file: File,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        let identity = crate::rooted::Metadata::from_open_file(&file)?;
        let file_identity = crate::observation::query::query_observation(
            &file,
            crate::observation::LocalObservationRequirement::Optional,
        )
        .ok()
        .and_then(|observation| observation.identity().cloned());
        let creation_parent = sandbox_path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        Ok(Self {
            core: LocalTempResourceCore::new(
                path.clone(),
                creation_parent,
                LocalTempResourceBackend::Rooted(RootedTempResourceBackend {
                    root,
                    relative_path: path,
                    sandbox_path,
                    identity,
                }),
                symlink_policy,
            ),
            file: Some(file),
            access_state: LocalTempAccessState::Writable,
            file_identity,
        })
    }

    /// Returns the namespace-absolute generated path.
    ///
    /// # Returns
    ///
    /// The generated path in the creating filesystem's namespace.
    #[must_use]
    #[inline]
    pub fn path(&self) -> &Path {
        &self.core.path
    }

    /// Returns the creation-time parent directory of the private sandbox.
    ///
    /// The path is absolute in the same namespace as [`Self::path`]. For a
    /// Rooted filesystem it uses virtual-root coordinates and never exposes
    /// the host path behind that root. The returned snapshot is retained for
    /// the resource's lifetime and does not prove that the directory still
    /// exists or is accessible. Querying it performs no I/O.
    ///
    /// # Returns
    ///
    /// The creation-time namespace-absolute parent directory.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use qubit_local_files::LocalFileSystem;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let filesystem = LocalFileSystem::host()?;
    /// let temporary = filesystem.create_temp_file()?;
    /// assert!(temporary.path().starts_with(temporary.creation_parent()));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    #[inline]
    pub fn creation_parent(&self) -> &Path {
        &self.core.creation_parent
    }

    /// Returns the current source authority, independently of earlier
    /// publication.
    ///
    /// # Returns
    ///
    /// The source lifecycle state, including whether cleanup remains required.
    #[inline]
    pub const fn source_state(&self) -> LocalTempSourceState {
        self.core.source_state()
    }

    /// Returns the mutable open file handle, or an error after [`Self::close`]
    /// or when source authority no longer permits file operations.
    ///
    /// # Returns
    ///
    /// A mutable reference to the writable open file handle.
    ///
    /// # Errors
    ///
    /// Returns the underlying authority error, or `BrokenPipe` when the handle
    /// has already been closed.
    #[inline]
    pub fn as_file_mut(&mut self) -> Result<&mut File> {
        if self.access_state != LocalTempAccessState::Writable {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "temporary file is not writable",
            ));
        }
        self.core.ensure_publishable()?;
        self.file.as_mut().ok_or_else(closed_file_error)
    }

    /// Writes vectored bytes to the still-open temporary file.
    ///
    /// # Parameters
    ///
    /// `buffers` contains the byte slices to write in order.
    ///
    /// # Returns
    ///
    /// The number of bytes written, or an I/O error.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is closed, authority is invalid, or
    /// the vectored write fails.
    #[inline]
    pub fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> Result<usize> {
        self.as_file_mut()?;
        let slices = crate::vectored::byte_slices(buffers)?;
        Output::write_vectored(self, &slices)
    }

    /// Closes the file I/O handle while retaining cleanup and persistence
    /// responsibility.
    ///
    /// # Side Effects
    ///
    /// Drops the open file handle, if present. Repeated calls have no effect.
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
    ///
    /// # Returns
    ///
    /// `Ok(())` after releasing the file and sandbox, or the cleanup error.
    ///
    /// # Errors
    ///
    /// Returns a structured cleanup error containing the affected path and
    /// native cause when identity checking or removal fails, or cleanup is
    /// unsafe.
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
    ///
    /// # Returns
    ///
    /// The publication outcome on success, or an error retaining this resource
    /// and its publication state.
    ///
    /// # Errors
    ///
    /// Returns an error if publication fails; the error records the failure
    /// stage and retains this resource for recovery.
    #[inline]
    pub fn keep(self) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        let requested_target = self.core.path.clone();
        if self.access_state != LocalTempAccessState::Writable {
            return Err(self.persist_error(
                Error::new(ErrorKind::PermissionDenied, "sealed temporary file cannot be persisted"),
                requested_target,
                None,
                LocalPersistStage::InstallDestination,
                LocalPersistFailureState::NotPublished,
            ));
        }
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
    ///
    /// # Parameters
    ///
    /// `target` is a nonempty namespace-absolute destination.
    ///
    /// # Returns
    ///
    /// The publication outcome, or an error retaining this resource and its
    /// publication state.
    ///
    /// # Errors
    ///
    /// Returns an error if target resolution or publication fails; the error
    /// retains this resource and reports publication certainty.
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
    ///
    /// # Parameters
    ///
    /// `target` is a nonempty namespace-absolute destination. `options` selects
    /// replacement, parent creation, and durability behavior.
    ///
    /// # Returns
    ///
    /// The achieved publication outcome, or an error retaining this resource
    /// and reporting the failure stage and publication certainty.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid targets, authority or policy failures,
    /// parent preparation, installation, or synchronization. It retains this
    /// resource and records the failure stage and publication certainty.
    #[inline]
    pub fn persist_with(
        self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path(target.as_ref(), None, options)
    }

    /// Seals this temporary file and returns an owner without write access.
    ///
    /// This consumes the writer owner. If sealing fails, the returned error
    /// retains it; call [`LocalTempSealError::into_resource`] to recover the
    /// `LocalTempFile` for inspection, cleanup, or another attempt. The error's
    /// [`LocalTempSealError::access_state`] reports whether access is still
    /// writable, read-only, or indeterminate: permission changes may already
    /// have taken effect before a later step fails.
    ///
    /// `file_sync` controls synchronization of file contents and metadata.
    /// It does not synchronize the containing directory or promise namespace
    /// durability. A successful seal removes the public write handle, but it
    /// does not make the file an immutable snapshot against other processes.
    ///
    /// # Parameters
    ///
    /// `options` selects the file synchronization requirement.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use qubit_io::{Output, Streams};
    ///
    /// use qubit_local_files::{LocalFileSystem, LocalSealOptions};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let filesystem = LocalFileSystem::host()?;
    /// let mut temporary = filesystem.create_temp_file()?;
    /// temporary.write_fully(b"sealed data")?;
    /// let mut sealed = temporary.seal_with_options(&LocalSealOptions::default())?;
    /// let observation = sealed.observation()?;
    /// assert_eq!(observation.metadata().len(), 11);
    /// let mut reader = filesystem.open_reader(sealed.path())?;
    /// let mut bytes = Vec::new();
    /// Streams::copy_input_to_output(&mut reader, &mut bytes)?;
    /// assert_eq!(bytes, b"sealed data");
    /// sealed.cleanup()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Returns
    ///
    /// A sealed owner with a read-only handle, or an error retaining the
    /// original resource for recovery.
    ///
    /// # Errors
    ///
    /// Returns an error if identity verification, synchronization, access
    /// restriction, or read-only reopen fails. The error retains the resource
    /// and reports the access state that may already have taken effect.
    pub fn seal_with_options(
        mut self,
        options: &LocalSealOptions,
    ) -> std::result::Result<LocalSealedTempFile, LocalTempSealError> {
        let result = (|| {
            let file = self.file.as_ref().ok_or_else(closed_file_error)?;
            let expected = crate::observation::query::query_observation(
                file,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(crate::LocalFileError::into_io_error)?
            .identity()
                == self.file_identity.as_ref();
            if self.file_identity.is_none() {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "temporary file identity is unavailable",
                ));
            }
            if !expected {
                return Err(Error::new(ErrorKind::InvalidInput, "temporary file identity changed"));
            }
            if options.file_sync() != LocalFileSyncRequirement::NotRequired {
                #[cfg(feature = "test-support")]
                if crate::local::take_test_support("temp-seal-file-sync-before") {
                    return Err(crate::local::test_fault_error());
                }
                match file.sync_all() {
                    Ok(()) => {}
                    Err(error) if options.file_sync() == LocalFileSyncRequirement::Required => {
                        return Err(error);
                    }
                    Err(_) => {}
                }
            }
            self.access_state = LocalTempAccessState::Indeterminate;
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support("temp-seal-access-change") {
                return Err(crate::local::test_fault_error());
            }
            #[cfg(unix)]
            crate::temp::internal::privacy_unix::apply_current_user_access(file, false, true)?;
            #[cfg(windows)]
            {
                let access_file = match &self.core.backend {
                    LocalTempResourceBackend::Host(_) => {
                        crate::temp::internal::privacy_windows::open_existing_host_file(self.path())?
                    }
                    LocalTempResourceBackend::Rooted(rooted) => {
                        let relative = crate::LocalRelativePath::new(&rooted.relative_path)
                            .map_err(|error| Error::new(ErrorKind::InvalidInput, error))?;
                        rooted.root.open_existing_file_for_security(relative.as_path())?
                    }
                };
                let access_identity = crate::observation::query::query_observation(
                    &access_file,
                    crate::observation::LocalObservationRequirement::Required,
                )
                .map_err(crate::LocalFileError::into_io_error)?
                .identity()
                .cloned();
                if access_identity.as_ref() != self.file_identity.as_ref() {
                    return Err(Error::new(ErrorKind::InvalidInput, "temporary file identity changed"));
                }
                crate::temp::internal::privacy_windows::apply_current_user_access(&access_file, false, true)?;
            }
            self.access_state = LocalTempAccessState::ReadOnly;
            if options.file_sync() == LocalFileSyncRequirement::Required {
                #[cfg(feature = "test-support")]
                if crate::local::take_test_support("temp-seal-file-sync-after-access") {
                    return Err(crate::local::test_fault_error());
                }
                file.sync_all()?;
            }
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support("temp-seal-readonly-reopen") {
                return Err(crate::local::test_fault_error());
            }
            let readonly = match &self.core.backend {
                LocalTempResourceBackend::Host(_) => {
                    crate::local::open_native_reader_path(self.path(), &crate::read::OpenOptions::default())?
                }
                LocalTempResourceBackend::Rooted(rooted) => {
                    let relative = crate::LocalRelativePath::new(&rooted.relative_path)
                        .map_err(|error| Error::new(ErrorKind::InvalidInput, error))?;
                    rooted
                        .root
                        .open_reader(&relative, &crate::read::OpenOptions::default())?
                }
            };
            let reopened_matches = crate::observation::query::query_observation(
                &readonly,
                crate::observation::LocalObservationRequirement::Required,
            )
            .map_err(crate::LocalFileError::into_io_error)?
            .identity()
                == self.file_identity.as_ref();
            if !reopened_matches {
                self.access_state = LocalTempAccessState::Indeterminate;
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "read-only handle is a different file",
                ));
            }
            self.file = Some(readonly);
            Ok(())
        })();
        if let Err(error) = result {
            let state = self.access_state;
            let cause =
                LocalFileError::from_io(LocalFileOperation::Commit, Some(self.path().to_path_buf()), None, error)
                    .with_temp_recovery(super::LocalTempRecovery::from_file(self));
            return Err(LocalTempSealError::new(cause, state));
        }
        Ok(LocalSealedTempFile::new(self))
    }

    /// Persists a relative target against an explicit namespace-absolute base.
    ///
    /// `base` must be an existing directory without dot or parent components;
    /// `target` must be nonempty and strictly relative. Both are interpreted
    /// through the creating authority and its captured symlink policy. This
    /// never reads the process PWD. Invalid parameters return `ResolveTarget`
    /// before source synchronization or closing. Other stages and publication
    /// guarantees match [`Self::persist_with`]. Errors retain the resource.
    ///
    /// # Parameters
    ///
    /// `base` must be an existing namespace-absolute directory. `target` must
    /// be a nonempty relative path. `options` selects replacement, parent
    /// creation, and durability behavior.
    ///
    /// # Returns
    ///
    /// The achieved publication outcome, or an error retaining this resource
    /// and reporting the failure stage and publication certainty.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths, authority or policy failures, parent
    /// preparation, installation, or synchronization. It retains this resource
    /// and records the failure stage and publication certainty.
    pub fn persist_at(
        self,
        base: &Path,
        target: &Path,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path(target, Some(base), options)
            .map_err(|error| error.with_current_directory(base.to_path_buf()))
    }

    /// Queries the identity of the currently open handle without requiring it.
    ///
    /// Returns `None` when the handle is closed or optional native inspection
    /// is unavailable.
    ///
    /// # Returns
    ///
    /// `Some(identity)` when optional inspection obtains an identity, or
    /// `None` when the handle is closed or inspection is unavailable.
    pub(crate) fn current_identity(&self) -> Option<crate::observation::LocalFileIdentity> {
        self.file.as_ref().and_then(|file| {
            crate::observation::query::query_observation(
                file,
                crate::observation::LocalObservationRequirement::Optional,
            )
            .ok()
            .and_then(|observation| observation.identity().cloned())
        })
    }

    /// Binds public paths and records the creating filesystem PWD for
    /// diagnostics. Returns a contextual path-resolution error on invalid
    /// namespace input; consuming failure drops this guard and attempts
    /// cleanup.
    ///
    /// # Parameters
    ///
    /// `resolver` supplies the namespace authority and path context to bind.
    ///
    /// # Returns
    ///
    /// The guard bound to the supplied namespace context.
    ///
    /// # Errors
    ///
    /// Returns a contextual resolution error when namespace input is invalid.
    pub(crate) fn bind_namespace(mut self, resolver: LocalPathResolver) -> LocalResult<Self> {
        self.core.bind_namespace(resolver, LocalFileOperation::CreateTempFile)?;
        Ok(self)
    }

    /// Returns the authority-local sandbox path used for cleanup diagnostics.
    ///
    /// # Returns
    ///
    /// The sandbox path in the retained authority's namespace.
    #[inline]
    fn cleanup_path(&self) -> PathBuf {
        self.core.cleanup_path()
    }

    /// Persists the file to a resolved public-API target path.
    /// Implements the ownership, guarantee, and failure contract of
    /// [`Self::persist_with`], using only the retained authority and explicit
    /// base.
    ///
    /// # Parameters
    ///
    /// `target` is the requested destination. `base` is an optional explicit
    /// namespace-absolute directory. `options` selects publication behavior.
    ///
    /// # Returns
    ///
    /// The result of persistence through the bound authority.
    ///
    /// # Errors
    ///
    /// Returns a staged persistence error retaining this resource.
    fn persist_with_path(
        self,
        target: &Path,
        base: Option<&Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path_before_reopen(target, base, options, || {})
    }

    /// Persists after running `before_reopen` at the closed-source reopen
    /// boundary. The public persistence path supplies a no-op action.
    ///
    /// # Type Parameters
    ///
    /// `F` is a one-shot action invoked at the closed-source reopen boundary.
    ///
    /// # Parameters
    ///
    /// `target` is the requested publication target. `base` is an optional
    /// explicit namespace base. `options` selects publication and durability
    /// policy. `before_reopen` runs after source identity precheck and before
    /// opening a closed source through its authority.
    ///
    /// # Returns
    /// The persistence outcome or a recoverable error retaining this resource.
    ///
    /// # Errors
    ///
    /// Returns a staged persistence error while retaining this resource; the
    /// source may have been closed and publication may have completed.
    fn persist_with_path_before_reopen<F>(
        mut self,
        target: &Path,
        base: Option<&Path>,
        options: LocalPersistOptions,
        before_reopen: F,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>>
    where
        F: FnOnce(),
    {
        let requested_target = target.to_path_buf();
        if self.access_state != LocalTempAccessState::Writable {
            return Err(self.persist_error(
                Error::new(ErrorKind::PermissionDenied, "sealed temporary file cannot be persisted"),
                requested_target,
                None,
                LocalPersistStage::InstallDestination,
                LocalPersistFailureState::NotPublished,
            ));
        }
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

        let file_durable = match self.synchronize_source_before_reopen(options.durability(), before_reopen) {
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
    /// Runs an action between source-path precheck and closed-handle reopen.
    ///
    /// # Type Parameters
    ///
    /// `F` is a one-shot action invoked before reopening a closed source.
    ///
    /// # Parameters
    ///
    /// `durability` selects whether synchronization is skipped, preferred, or
    /// required. `before_reopen` runs once before reopening a closed source.
    ///
    /// # Returns
    ///
    /// `true` when synchronization succeeds, or `false` when skipped or when
    /// preferred durability cannot be achieved.
    ///
    /// # Errors
    ///
    /// Returns required synchronization, reopen, or source-identity failures.
    fn synchronize_source_before_reopen<F>(
        &self,
        durability: LocalDurabilityRequirement,
        before_reopen: F,
    ) -> Result<bool>
    where
        F: FnOnce(),
    {
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
        let mut before_reopen = Some(before_reopen);
        let mut synchronize = || -> Result<bool> {
            if let Some(file) = self.file.as_ref() {
                return self.synchronize_open_file(file, durability);
            }
            before_reopen
                .take()
                .expect("closed-source reopen action should run only once")();
            match &self.core.backend {
                LocalTempResourceBackend::Host(_) => {
                    let file =
                        crate::local::open_native_reader_path(&self.core.path, &crate::read::OpenOptions::default())?;
                    self.synchronize_open_file(&file, durability)
                }
                LocalTempResourceBackend::Rooted(rooted) => {
                    let path = LocalRelativePath::new(&rooted.relative_path)?;
                    let file = rooted.root.open_probe_file(&path)?;
                    self.synchronize_open_file(&file, durability)
                }
            }
        };
        match durability {
            LocalDurabilityRequirement::Required => {
                synchronize().and_then(|synced| if synced { Ok(true) } else { Err(sync_identity_error()) })
            }
            LocalDurabilityRequirement::Preferred => synchronize(),
            LocalDurabilityRequirement::NotRequired => Ok(false),
        }
    }

    /// Validates the reopened handle before applying the sync fallback.
    ///
    /// # Parameters
    ///
    /// `file` is the candidate handle to validate and synchronize.
    /// `durability` selects whether synchronization is preferred or required.
    ///
    /// # Returns
    /// `true` after synchronization, or `false` only when a preferred
    /// `sync_all` operation fails after the handle has been verified.
    ///
    /// # Errors
    /// Returns identity-inspection failures for both modes and required
    /// synchronization failures.
    fn synchronize_open_file(&self, file: &File, durability: LocalDurabilityRequirement) -> Result<bool> {
        #[cfg(all(test, unix))]
        if FAIL_NEXT_TEMP_SOURCE_IDENTITY_QUERY.with(|selected| selected.replace(false)) {
            return Err(Error::other("injected source identity query error"));
        }
        let matches = match &self.core.backend {
            LocalTempResourceBackend::Host(backend) => backend.identity.matches_file(file)?,
            LocalTempResourceBackend::Rooted(backend) => {
                let opened = crate::rooted::Metadata::from_open_file(file)?;
                backend.identity.is_same_file(&opened)
            }
        };
        if !matches {
            return Err(sync_identity_error());
        }
        #[cfg(all(test, unix))]
        let sync_result = if FAIL_NEXT_TEMP_SOURCE_SYNC.with(|selected| selected.replace(false)) {
            Err(Error::other("injected source sync error"))
        } else {
            file.sync_all()
        };
        #[cfg(not(all(test, unix)))]
        let sync_result = file.sync_all();
        match sync_result {
            Ok(()) => Ok(true),
            Err(_) if durability == LocalDurabilityRequirement::Preferred => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Removes the resource using the retained backend rather than a diagnostic
    /// path.
    /// Rechecks identity before removal and propagates inspection/removal
    /// errors. An identity mismatch marks namespace state indeterminate.
    ///
    /// # Returns
    ///
    /// `Ok(())` after removing the owned file entry.
    ///
    /// # Errors
    ///
    /// Returns an identity inspection or native removal error.
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
                #[cfg(windows)]
                rooted.root.remove_owned_read_only_file(&path, &rooted.identity)?;
                #[cfg(not(windows))]
                rooted.root.remove_file(&path)?;
                Ok(())
            }
        }
    }

    /// Removes the now-empty private sandbox.
    /// Propagates native removal errors, including a non-empty sandbox.
    ///
    /// # Returns
    ///
    /// `Ok(())` after removing the empty sandbox.
    ///
    /// # Errors
    ///
    /// Returns a native error when the sandbox cannot be removed.
    fn release_sandbox(&self) -> Result<()> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("temp-file-sandbox-remove") {
            return Err(crate::local::test_fault_error());
        }
        self.core.release_sandbox()
    }

    /// Builds a persistence failure with the resource's creation-time PWD.
    ///
    /// # Parameters
    ///
    /// `error` is the failure cause; `requested_target` and `resolved_target`
    /// record destination context; `stage` identifies the failed operation;
    /// `publication` records whether installation may have occurred.
    ///
    /// # Returns
    ///
    /// A persistence error retaining this resource and its publication state.
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
    ///
    /// # Parameters
    ///
    /// `error` is the structured failure to contextualize with the creation
    /// namespace.
    ///
    /// # Returns
    ///
    /// The error augmented with the creation-time namespace context.
    fn contextualize_error(&self, error: LocalFileError) -> LocalFileError {
        self.core.contextualize_error(error)
    }

    /// Rejects namespace cleanup after an indeterminate native publication
    /// attempt.
    ///
    /// # Returns
    ///
    /// `Ok(())` when cleanup is safe to attempt.
    ///
    /// # Errors
    ///
    /// Returns an error when publication state makes cleanup unsafe.
    #[inline]
    fn ensure_cleanup_safe(&self) -> Result<()> {
        self.core.ensure_cleanup_safe()
    }

    /// Rejects operations when the authority path no longer names this file.
    /// A mismatch marks the state indeterminate and returns `InvalidInput`;
    /// failed identity inspection also marks the state indeterminate while
    /// preserving its native error.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the retained path still identifies the original file.
    ///
    /// # Errors
    ///
    /// Returns an identity mismatch or inspection error and marks the source
    /// state indeterminate.
    fn ensure_identity_matches(&mut self) -> Result<()> {
        let result = self.core.ensure_identity_matches();
        if self.core.state == LocalTempResourceState::Indeterminate {
            self.close();
        }
        result
    }

    /// Records whether a failed native install proves the source remains owned.
    ///
    /// # Parameters
    ///
    /// `error` is the native installation failure used to infer publication
    /// certainty.
    ///
    /// # Returns
    ///
    /// The publication state supported by the native error.
    #[inline]
    fn record_native_persist_failure(&mut self, error: &Error) -> LocalPersistFailureState {
        self.core.record_native_persist_failure(error)
    }
}

/// Synchronizes both Host rename parents and every created destination parent.
///
/// # Parameters
///
/// `source` and `target` are the rename endpoints. `created_parents` lists
/// destination directories created for publication. `durability` selects the
/// synchronization requirement.
///
/// # Returns
///
/// Whether every required parent synchronization succeeded.
///
/// # Errors
///
/// Returns a native synchronization error when durability is required.
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
///
/// # Parameters
///
/// `root` supplies authority; `source` and `target` are rooted rename
/// endpoints. `durability` selects the synchronization requirement.
///
/// # Returns
///
/// Whether every required parent synchronization succeeded.
///
/// # Errors
///
/// Returns a native synchronization or rooted-path validation error when
/// durability is required.
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
///
/// # Parameters
///
/// `durability` selects the synchronization requirement. `synchronize` runs
/// the destination synchronization operation once when required by policy.
///
/// # Returns
///
/// `false` when synchronization is skipped or preferred synchronization
/// fails; otherwise `true`.
///
/// # Errors
///
/// Returns the synchronization error in required mode or an injected test
/// failure when test support requests one.
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

impl Output for LocalTempFile {
    /// The byte type accepted by this output.
    type Item = u8;

    /// Writes the supplied byte slices to the open temporary file.
    ///
    /// # Parameters
    ///
    /// `buffers` contains the byte slices to write in order.
    ///
    /// # Returns
    ///
    /// The number of bytes written, or an I/O error.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is closed, authority is invalid, or
    /// the write fails.
    fn write_vectored(&mut self, buffers: &[&[u8]]) -> Result<usize> {
        Output::write_vectored(self.as_file_mut()?, buffers)
    }

    /// Writes the requested valid range to the still-open temporary file.
    ///
    /// # Parameters
    ///
    /// `input` is the source byte slice; `index` and `count` define the valid
    /// range written to the file.
    ///
    /// # Returns
    ///
    /// The number of bytes written, or an I/O error.
    ///
    /// # Errors
    ///
    /// Returns an error when the file handle is unavailable or the write
    /// fails.
    ///
    /// # Safety
    ///
    /// `index..index + count` must be a valid range inside `input`, and the
    /// addition must not overflow.
    #[inline]
    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> Result<usize> {
        Output::write(self.as_file_mut()?, &input[index..index + count])
    }

    /// Flushes the still-open temporary file.
    ///
    /// # Returns
    ///
    /// `Ok(())` after flushing, or an I/O error.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is closed, authority is invalid, or
    /// flushing fails.
    #[inline]
    fn flush(&mut self) -> Result<()> {
        Output::flush(self.as_file_mut()?)
    }
}

impl Seek for LocalTempFile {
    /// Seeks the still-open temporary file.
    ///
    /// # Parameters
    ///
    /// `position` selects the new file cursor position.
    ///
    /// # Returns
    ///
    /// The resulting cursor position, or an I/O error.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is closed, authority is invalid, or
    /// the seek fails.
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

// Defines the sealed temporary-file owner and its lifecycle API.
#[path = "local_sealed_temp_file.rs"]
mod local_sealed_temp_file;

pub use local_sealed_temp_file::LocalSealedTempFile;

/// Builds the error used when the opened synchronization object is not source.
#[must_use]
#[inline]
fn sync_identity_error() -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        "temporary file handle does not identify the retained source",
    )
}

#[cfg(all(test, unix))]
mod sync_identity_tests {
    use std::ffi::CString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::FileTypeExt;
    use std::path::Path;
    use std::process::Child;
    use std::process::Command;
    use std::time::Duration;
    use std::time::Instant;

    use crate::LocalDurabilityRequirement;
    use crate::LocalFileSystem;
    use crate::LocalPersistFailureState;
    use crate::LocalPersistOptions;
    use crate::LocalPersistStage;
    use crate::LocalTempFileOptions;
    use crate::outcome::LocalTempSourceState;

    struct ChildGuard(Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    #[test]
    fn test_closed_reopen_rejects_path_replacement_after_source_precheck() {
        for rooted in [false, true] {
            for durability in [
                LocalDurabilityRequirement::Required,
                LocalDurabilityRequirement::Preferred,
            ] {
                let directory = tempfile::tempdir().expect("private parent should be created");
                let filesystem = if rooted {
                    LocalFileSystem::rooted(directory.path()).expect("root authority should open")
                } else {
                    LocalFileSystem::host().expect("host authority should open")
                };
                let parent = if rooted { Path::new("/") } else { directory.path() };
                let mut temporary = filesystem
                    .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
                    .expect("temporary source should be created");
                let namespace_source = temporary.path().to_path_buf();
                let source = if rooted {
                    directory.path().join(
                        namespace_source
                            .strip_prefix(Path::new("/"))
                            .expect("rooted source should be virtual-absolute"),
                    )
                } else {
                    namespace_source
                };
                let replacement = directory.path().join("replacement");
                fs::write(&replacement, b"replacement entity")
                    .expect("replacement entity should be created before swapping");
                temporary.close();

                let target = directory.path().join("published");
                let namespace_target = if rooted {
                    Path::new("/published")
                } else {
                    target.as_path()
                };
                let mut seam_reached = false;
                let error = temporary
                    .persist_with_path_before_reopen(
                        namespace_target,
                        None,
                        LocalPersistOptions::new().with_durability(durability),
                        || {
                            fs::remove_file(&source).expect("prechecked source should be removable");
                            fs::rename(&replacement, &source)
                                .expect("replacement should take the prechecked source path");
                            seam_reached = true;
                        },
                    )
                    .expect_err("reopened handle identity mismatch must reject persistence");

                assert!(seam_reached, "replacement seam must run");
                assert_eq!(LocalPersistStage::SynchronizeSource, error.stage());
                assert_eq!(LocalPersistFailureState::NotPublished, error.state());
                assert_eq!(LocalTempSourceState::Owned, error.source_state());
                assert!(
                    error
                        .error()
                        .to_string()
                        .contains("temporary file handle does not identify the retained source"),
                    "failure must come from the reopened handle identity comparison",
                );
                assert!(!target.exists(), "replacement must not be published");
                assert_eq!(
                    b"replacement entity",
                    fs::read(&source)
                        .expect("replacement must remain at the source path")
                        .as_slice(),
                );
            }
        }
    }

    #[test]
    fn test_preferred_closed_reopen_error_is_not_treated_as_sync_degradation() {
        for rooted in [false, true] {
            let directory = tempfile::tempdir().expect("private parent should be created");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("host authority should open")
            };
            let parent = if rooted { Path::new("/") } else { directory.path() };
            let mut temporary = filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
                .expect("temporary source should be created");
            let namespace_source = temporary.path().to_path_buf();
            let source = if rooted {
                directory
                    .path()
                    .join(namespace_source.strip_prefix(Path::new("/")).expect("virtual source"))
            } else {
                namespace_source
            };
            temporary.close();
            let relocated = directory.path().join("relocated-source");
            let target = directory.path().join("published");
            let namespace_target = if rooted {
                Path::new("/published")
            } else {
                target.as_path()
            };
            let error = temporary
                .persist_with_path_before_reopen(
                    namespace_target,
                    None,
                    LocalPersistOptions::new().with_durability(LocalDurabilityRequirement::Preferred),
                    || fs::rename(&source, &relocated).expect("retain source under another fixture name"),
                )
                .expect_err("Preferred must propagate source reopen failure");
            assert_eq!(LocalPersistStage::SynchronizeSource, error.stage());
            assert_eq!(LocalPersistFailureState::NotPublished, error.state());
            assert_eq!(LocalTempSourceState::Owned, error.source_state());
            assert!(!target.exists(), "reopen failure must prevent target installation");
            assert!(relocated.exists(), "the fixture retains the original source entity");
        }
    }

    #[test]
    fn test_preferred_closed_identity_query_error_is_not_treated_as_sync_degradation() {
        for rooted in [false, true] {
            let directory = tempfile::tempdir().expect("private parent should be created");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("host authority should open")
            };
            let parent = if rooted { Path::new("/") } else { directory.path() };
            let mut temporary = filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
                .expect("temporary source should be created");
            let namespace_source = temporary.path().to_path_buf();
            let source = if rooted {
                directory
                    .path()
                    .join(namespace_source.strip_prefix(Path::new("/")).expect("virtual source"))
            } else {
                namespace_source
            };
            temporary.close();
            let target = directory.path().join("published");
            let namespace_target = if rooted {
                Path::new("/published")
            } else {
                target.as_path()
            };
            let error = temporary
                .persist_with_path_before_reopen(
                    namespace_target,
                    None,
                    LocalPersistOptions::new().with_durability(LocalDurabilityRequirement::Preferred),
                    super::inject_temp_source_identity_query_error,
                )
                .expect_err("Preferred must propagate source identity inspection failure");
            assert_eq!(LocalPersistStage::SynchronizeSource, error.stage());
            assert_eq!(LocalPersistFailureState::NotPublished, error.state());
            assert_eq!(LocalTempSourceState::Owned, error.source_state());
            assert!(
                error
                    .error()
                    .to_string()
                    .contains("injected source identity query error"),
                "identity inspection error is preserved"
            );
            assert!(
                !target.exists(),
                "identity inspection failure must prevent installation"
            );
            assert!(source.exists(), "source remains owned after rejection");
        }
    }

    #[test]
    fn test_verified_sync_failure_degrades_only_for_preferred() {
        for durability in [
            LocalDurabilityRequirement::Required,
            LocalDurabilityRequirement::Preferred,
        ] {
            let directory = tempfile::tempdir().expect("private parent should be created");
            let mut temporary = LocalFileSystem::host()
                .expect("host authority should open")
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(directory.path()))
                .expect("temporary source should be created");
            super::inject_temp_source_sync_error();
            let result = temporary.synchronize_source_before_reopen(durability, || {
                panic!("open source handle does not invoke the closed-reopen seam")
            });
            match durability {
                LocalDurabilityRequirement::Required => {
                    assert!(
                        result
                            .expect_err("required sync failure propagates")
                            .to_string()
                            .contains("injected source sync error")
                    );
                }
                LocalDurabilityRequirement::Preferred => {
                    assert!(!result.expect("only verified sync failure may degrade"));
                }
                LocalDurabilityRequirement::NotRequired => unreachable!(),
            }
            temporary.cleanup().expect("source cleanup remains available");
        }
    }

    #[test]
    fn test_closed_reopen_rejects_fifo_path_replacement_without_blocking() {
        const CHILD: &str = "QUBIT_LF_REV_Q01_PERSIST_CHILD";
        const DURABILITY: &str = "QUBIT_LF_REV_Q01_PERSIST_DURABILITY";
        if let Some(parent) = std::env::var_os(CHILD) {
            let parent = Path::new(&parent);
            let durability = match std::env::var(DURABILITY).as_deref() {
                Ok("preferred") => LocalDurabilityRequirement::Preferred,
                _ => LocalDurabilityRequirement::Required,
            };
            let mut temporary = LocalFileSystem::host()
                .expect("host authority should open")
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
                .expect("temporary source should be created");
            let source = temporary.path().to_path_buf();
            temporary.close();
            let target = parent.join("published");
            let mut seam_reached = false;
            let error = temporary
                .persist_with_path_before_reopen(
                    &target,
                    None,
                    LocalPersistOptions::new().with_durability(durability),
                    || {
                        fs::remove_file(&source).expect("source path should be removable");
                        let native = CString::new(source.as_os_str().as_bytes()).expect("fixture has no NUL");
                        // SAFETY: native is a live NUL-terminated fixture path.
                        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
                        seam_reached = true;
                    },
                )
                .expect_err("FIFO source replacement must be rejected");

            assert!(seam_reached, "replacement seam must run");
            assert_eq!(LocalPersistStage::SynchronizeSource, error.stage());
            assert_eq!(LocalPersistFailureState::NotPublished, error.state());
            assert_eq!(LocalTempSourceState::Owned, error.source_state());
            assert!(
                fs::symlink_metadata(&source)
                    .expect("FIFO replacement remains")
                    .file_type()
                    .is_fifo()
            );
            let marker = format!("persist-child-completed-{durability:?}");
            fs::write(parent.join(marker), b"rejected").expect("completion marker");
            return;
        }

        for (durability_name, durability) in [
            ("required", LocalDurabilityRequirement::Required),
            ("preferred", LocalDurabilityRequirement::Preferred),
        ] {
            let fixture = tempfile::tempdir().expect("parent owns fixture");
            let test_executable = std::env::current_exe().expect("unit test binary");
            let mut child = ChildGuard(
                Command::new(test_executable)
                    .args([
                        "--exact",
                        "temp::local_temp_file::sync_identity_tests::test_closed_reopen_rejects_fifo_path_replacement_without_blocking",
                        "--nocapture",
                    ])
                    .env(CHILD, fixture.path())
                    .env(DURABILITY, durability_name)
                    .spawn()
                    .expect("child should start"),
            );
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.0.try_wait().expect("child status") {
                    assert!(status.success(), "child failed: {status}");
                    let marker = format!("persist-child-completed-{durability:?}");
                    assert_eq!(
                        fs::read(fixture.path().join(marker)).expect("child executed and wrote completion marker"),
                        b"rejected",
                    );
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Q01 {durability_name} FIFO persist: child failed to return before watchdog"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
