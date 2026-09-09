// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup-owned temporary directories with host or rooted authority.
// qubit-style: allow coverage-cfg

use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::internal::LocalTempResourceBackend;
use super::internal::LocalTempResourceState;
use super::internal::RootedTempResourceBackend;
use super::internal::TempEntryIdentity;
use super::internal::generated_target;
use super::internal::prepare_host_parent;
use super::internal::prepare_persist_target;
use super::internal::prepare_rooted_parent;
use super::internal::validate_persist_base;
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
use crate::path::LocalFileSystemScope;
use crate::path::LocalPathResolver;

/// A temporary directory whose cleanup remains bound to its creating authority.
///
/// Cleanup rejects ordinary path replacement by checking the identity captured
/// at creation. The check and deletion are not atomic, so callers must exclude
/// untrusted concurrent mutation of the containing directory; identity reuse
/// and a check/delete race cannot be ruled out by this path-based API.
/// Persistence resolves intermediate symbolic links using the policy captured
/// by the creating [`crate::LocalFileSystem`], while replacing a final link
/// entry itself.
///
/// The directory is created inside a private generated sandbox. Cleanup
/// removes the directory tree and then the empty sandbox. [`Self::keep`]
/// atomically publishes the directory outside that sandbox.
///
/// # Examples
///
/// ```no_run
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut temporary = filesystem.create_temp_directory()?;
/// let child = temporary.child(std::path::Path::new("child.txt"))?;
/// assert!(child.ends_with("child.txt"));
/// temporary.cleanup()?;
/// # Ok(())
/// # }
/// ```
#[must_use = "use explicit cleanup to observe errors; drop only attempts cleanup"]
#[derive(Debug)]
pub struct LocalTempDirectory {
    /// Stable namespace-absolute path after public namespace binding.
    path: PathBuf,
    /// Authority and resource ownership state.
    backend: LocalTempResourceBackend,
    /// Native identity captured when the temporary directory was created.
    host_identity: Option<TempEntryIdentity>,
    /// Rooted identity captured through the opened root authority.
    rooted_identity: Option<crate::rooted::Metadata>,
    /// Namespace certainty governing cleanup and drop behavior.
    state: LocalTempResourceState,
    /// Symbolic-link policy retained for persistence targets.
    symlink_policy: LocalSymlinkPolicy,
    /// Creation-time PWD retained only for source and cleanup diagnostics.
    creation_current_directory: Option<PathBuf>,
}

impl LocalTempDirectory {
    /// Builds a host temporary directory from its already-bound path.
    /// Captures identity without following the final link. Native inspection
    /// failure leaves cleanup of the created directory and sandbox to the
    /// caller.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) fn host(path: PathBuf, sandbox_path: PathBuf, symlink_policy: LocalSymlinkPolicy) -> Result<Self> {
        Ok(Self {
            host_identity: Some(TempEntryIdentity::from_path(&path)?),
            rooted_identity: None,
            path,
            backend: LocalTempResourceBackend::Host(super::internal::HostTempResourceBackend { sandbox_path }),
            state: LocalTempResourceState::Owned,
            symlink_policy,
            creation_current_directory: None,
        })
    }

    /// Builds a rooted temporary directory from the retained root authority.
    /// Requires a previously validated authority-relative `path`. Native
    /// inspection failure leaves cleanup of the created directory and sandbox
    /// to the caller through the retained `root` authority.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) fn rooted(
        root: Arc<crate::rooted::Root>,
        path: PathBuf,
        sandbox_path: PathBuf,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        let relative = LocalRelativePath::new(&path).expect("rooted temporary path was validated at creation");
        Ok(Self {
            host_identity: None,
            rooted_identity: Some(root.symlink_metadata(&relative)?),
            path: path.clone(),
            backend: LocalTempResourceBackend::Rooted(RootedTempResourceBackend {
                root,
                relative_path: path,
                sandbox_path,
            }),
            state: LocalTempResourceState::Owned,
            symlink_policy,
            creation_current_directory: None,
        })
    }

    /// Returns the namespace-absolute generated path.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Removes the directory tree through the retained authority.
    ///
    /// Returns a structured error when identity inspection, tree removal, or
    /// empty-sandbox removal fails, or namespace certainty makes cleanup
    /// unsafe. Tree removal is incremental and cannot be rolled back. A
    /// sandbox-only failure can be retried; successful cleanup is idempotent.
    pub fn cleanup(&mut self) -> LocalResult<()> {
        self.ensure_cleanup_safe().map_err(|error| {
            self.contextualize_error(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.path.clone()),
                None,
                error,
            ))
        })?;
        if self.state == LocalTempResourceState::Owned {
            self.remove_resource().map_err(|error| {
                self.contextualize_error(LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.path.clone()),
                    None,
                    error,
                ))
            })?;
            self.state = LocalTempResourceState::SandboxPending;
        }
        if self.state == LocalTempResourceState::SandboxPending {
            self.release_sandbox().map_err(|error| {
                self.contextualize_error(LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.cleanup_path()),
                    None,
                    error,
                ))
            })?;
            self.state = LocalTempResourceState::Released;
        }
        Ok(())
    }

    /// Resolves one normal child component below this directory.
    /// Performs no I/O or existence check. Returns `InvalidInput` unless
    /// `child` is exactly one validated normal relative component.
    pub fn child(&self, child: &Path) -> Result<PathBuf> {
        let relative = LocalRelativePath::new(child)?;
        if relative.as_path().components().count() != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "temporary-directory child must be one normal component",
            ));
        }
        Ok(self.path.join(relative.as_path()))
    }

    /// Resolves a normal relative descendant below this directory.
    /// Performs no I/O or existence check. Returns a relative-path validation
    /// error for empty input, roots, prefixes, dots, parents, or native NUL.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn descendant(&self, descendant: &Path) -> Result<PathBuf> {
        let relative = LocalRelativePath::new(descendant)?;
        Ok(self.path.join(relative.as_path()))
    }

    /// Atomically publishes the directory to a generated sibling outside its
    /// private sandbox.
    /// Uses the default no-replacement policy of [`Self::persist_with`], with
    /// the same publication outcome and resource-retaining failure contract.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn keep(self) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        let requested_target = self.path.clone();
        let target = match generated_target(&requested_target) {
            Ok(target) => target,
            Err(error) => {
                return Err(self.persist_error(error, requested_target, None, LocalPersistStage::ResolveTarget));
            }
        };
        self.persist_with_path(&target, None, LocalPersistOptions::new())
    }

    /// Persists the directory without replacement through its creating
    /// authority.
    /// Uses [`Self::persist_with`] with default options and the same errors.
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn persist(
        self,
        target: impl AsRef<Path>,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with(target, LocalPersistOptions::new())
    }

    /// Persists the directory with an explicit replacement policy through its
    /// creating authority.
    ///
    /// Consumes this guard and requires a nonempty namespace-absolute target.
    /// Use [`Self::persist_at`] for relative targets with an explicit base.
    /// Invalid targets retain the original guard before synchronization.
    /// `options` controls replacement and parent creation.
    /// Required durability returns `RequirementNotMet` before publication;
    /// directory-content durability is not implemented. Successful publication
    /// returns an atomic rename outcome, `durable: false`, and any later
    /// sandbox cleanup error.
    ///
    /// Identity, path resolution, parent creation, policy, and native install
    /// failures retain the resource, stage, and publication certainty in
    /// `LocalPersistError`. Created parents are not rolled back; inspect the
    /// failure state before retry or cleanup.
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn persist_with(
        self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path(target.as_ref(), None, options)
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
        let input = match &self.backend {
            LocalTempResourceBackend::Host(_) => self.path.clone(),
            LocalTempResourceBackend::Rooted(rooted) => virtual_rooted_path(&rooted.relative_path),
        };
        self.path = resolver
            .resolve(&input)
            .map_err(|error| {
                let error = error.with_operation(LocalFileOperation::CreateTempDirectory);
                match resolver.current_directory() {
                    Some(current_directory) => error.with_current_directory(current_directory.to_path_buf()),
                    None => error,
                }
            })?
            .namespace_absolute()
            .to_path_buf();
        self.creation_current_directory = resolver.current_directory().map(Path::to_path_buf);
        Ok(self)
    }

    /// Persists the directory to a resolved public-API target path.
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
        let scope = match &self.backend {
            LocalTempResourceBackend::Host(_) => LocalFileSystemScope::Host,
            LocalTempResourceBackend::Rooted(_) => LocalFileSystemScope::Rooted,
        };
        let resolved_target = match prepare_persist_target(scope, base, target) {
            Ok(target) => target,
            Err(error) => {
                return Err(self.persist_error(
                    error.into_io_error(),
                    requested_target,
                    None,
                    LocalPersistStage::ResolveTarget,
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
            ));
        }
        if let Some(base) = base
            && let Err(error) = validate_persist_base(&self.backend, base, self.symlink_policy)
        {
            return Err(self.persist_error(
                error.into_io_error(),
                requested_target,
                Some(namespace_target),
                LocalPersistStage::ResolveTarget,
            ));
        }
        if let Err(error) = self.ensure_identity_matches() {
            return Err(self.persist_error(error, target.to_path_buf(), None, LocalPersistStage::InstallDestination));
        }
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(self.persist_error(
                std::io::Error::other("temporary directory namespace state is indeterminate"),
                target.to_path_buf(),
                None,
                LocalPersistStage::InstallDestination,
            ));
        }
        if options.durability() == LocalDurabilityRequirement::Required {
            return Err(self
                .persist_error(
                    Error::new(
                        ErrorKind::Unsupported,
                        "required temporary-directory content durability cannot be guaranteed",
                    ),
                    target.to_path_buf(),
                    None,
                    LocalPersistStage::SynchronizeSource,
                )
                .with_kind(LocalFileErrorKind::RequirementNotMet));
        }
        let authority_target = resolved_target.authority_relative().to_path_buf();
        match &self.backend {
            LocalTempResourceBackend::Host(_) => {
                let target = match crate::local::resolve_host_path(&authority_target, self.symlink_policy, false) {
                    Ok(target) => target,
                    Err(error) => {
                        return Err(self.persist_error(
                            error.into_io_error(),
                            requested_target,
                            Some(namespace_target),
                            LocalPersistStage::ResolveTarget,
                        ));
                    }
                };
                if let Err(error) = prepare_host_parent(&target, options.creates_parent()) {
                    return Err(self.persist_error(
                        error,
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::PrepareParent,
                    ));
                }
                let result = if options.overwrites() {
                    std::fs::rename(&self.path, &target)
                } else {
                    crate::local::move_directory_without_replacing(&self.path, &target)
                };
                if let Err(error) = result {
                    self.record_native_persist_failure(&error);
                    return Err(self.persist_error(
                        error,
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::InstallDestination,
                    ));
                }
                let cleanup_error = self.release_sandbox().err().map(|error| {
                    self.contextualize_error(LocalFileError::from_io(
                        LocalFileOperation::Cleanup,
                        Some(self.cleanup_path()),
                        None,
                        error,
                    ))
                });
                self.state = LocalTempResourceState::Released;
                Ok(LocalPersistOutcome::new(
                    namespace_target,
                    LocalPersistMethod::AtomicRename,
                    true,
                    false,
                    cleanup_error,
                ))
            }
            LocalTempResourceBackend::Rooted(rooted) => {
                let target = match LocalRelativePath::new(&authority_target) {
                    Ok(target) => target.as_path().to_path_buf(),
                    Err(error) => {
                        return Err(self.persist_error(
                            error,
                            requested_target,
                            Some(namespace_target),
                            LocalPersistStage::ResolveTarget,
                        ));
                    }
                };
                let source = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                let resolved = match crate::rooted_local_file_system::resolve_rooted_path(
                    &rooted.root,
                    &target,
                    self.symlink_policy,
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
                    ));
                }
                let result = if options.overwrites() {
                    rooted.root.rename(&source, &destination)
                } else {
                    rooted.root.rename_without_replacing(&source, &destination)
                };
                if let Err(error) = result {
                    self.record_native_persist_failure(&error);
                    return Err(self.persist_error(
                        error,
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::InstallDestination,
                    ));
                }
                let cleanup_error = self.release_sandbox().err().map(|error| {
                    self.contextualize_error(LocalFileError::from_io(
                        LocalFileOperation::Cleanup,
                        Some(self.cleanup_path()),
                        None,
                        error,
                    ))
                });
                self.state = LocalTempResourceState::Released;
                Ok(LocalPersistOutcome::new(
                    namespace_target,
                    LocalPersistMethod::AtomicRename,
                    true,
                    false,
                    cleanup_error,
                ))
            }
        }
    }

    /// Removes the resource using the retained backend rather than a diagnostic
    /// path.
    /// Rechecks identity before recursive removal and propagates inspection or
    /// removal errors. Earlier deletions remain after a later failure.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    fn remove_resource(&mut self) -> Result<()> {
        self.ensure_identity_matches()?;
        match &self.backend {
            LocalTempResourceBackend::Host(_) => {
                std::fs::remove_dir_all(&self.path)?;
                Ok(())
            }
            LocalTempResourceBackend::Rooted(rooted) => {
                let path = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                rooted.root.remove_tree(&path)?;
                Ok(())
            }
        }
    }

    /// Removes the now-empty private sandbox.
    /// Propagates native removal errors, including a non-empty sandbox.
    fn release_sandbox(&self) -> Result<()> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("temp-directory-sandbox-remove") {
            return Err(crate::local::test_fault_error());
        }
        match &self.backend {
            LocalTempResourceBackend::Host(host) => std::fs::remove_dir(&host.sandbox_path),
            LocalTempResourceBackend::Rooted(rooted) => {
                let sandbox = LocalRelativePath::new(&rooted.sandbox_path)
                    .expect("rooted temporary sandbox path was validated at creation");
                rooted.root.remove_empty_dir(&sandbox)
            }
        }
    }

    /// Returns the authority-local sandbox path used for cleanup diagnostics.
    fn cleanup_path(&self) -> PathBuf {
        match &self.backend {
            LocalTempResourceBackend::Host(host) => host.sandbox_path.clone(),
            LocalTempResourceBackend::Rooted(rooted) => virtual_rooted_path(&rooted.sandbox_path),
        }
    }

    /// Builds a persistence failure with the resource's creation-time PWD.
    fn persist_error(
        self,
        error: Error,
        requested_target: PathBuf,
        resolved_target: Option<PathBuf>,
        stage: LocalPersistStage,
    ) -> LocalPersistError<Self> {
        let current_directory = self.creation_current_directory.clone();
        let error = LocalPersistError::new(error, self, requested_target, resolved_target, stage);
        match current_directory {
            Some(current_directory) => error.with_current_directory(current_directory),
            None => error,
        }
    }

    /// Attaches the resource's creation-time PWD to a structured error.
    fn contextualize_error(&self, error: LocalFileError) -> LocalFileError {
        match &self.creation_current_directory {
            Some(current_directory) => error.with_current_directory(current_directory.clone()),
            None => error,
        }
    }

    /// Rejects namespace cleanup after an indeterminate native publication
    /// attempt.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    fn ensure_cleanup_safe(&self) -> Result<()> {
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(std::io::Error::other(
                "temporary directory namespace state is indeterminate; cleanup is unsafe",
            ));
        }
        Ok(())
    }

    /// Rejects operations when the authority path no longer names this
    /// directory.
    /// A mismatch marks the state indeterminate and returns `InvalidInput`;
    /// native inspection errors propagate without changing the state.
    fn ensure_identity_matches(&mut self) -> Result<()> {
        let matches = match &self.backend {
            LocalTempResourceBackend::Host(_) => self
                .host_identity
                .as_ref()
                .expect("host temporary directory must retain host identity")
                .matches_path(&self.path),
            LocalTempResourceBackend::Rooted(rooted) => rooted
                .root
                .symlink_metadata(
                    &LocalRelativePath::new(&rooted.relative_path)
                        .expect("rooted temporary path was validated at creation"),
                )
                .map(|metadata| {
                    metadata.is_same_file(
                        self.rooted_identity
                            .as_ref()
                            .expect("rooted temporary directory must retain rooted identity"),
                    )
                }),
        };
        match matches {
            Ok(true) => Ok(()),
            Ok(false) => {
                self.state = LocalTempResourceState::Indeterminate;
                Err(Error::new(
                    ErrorKind::InvalidInput,
                    "temporary directory path no longer names the created entry",
                ))
            }
            Err(error) => Err(error),
        }
    }

    /// Records whether a failed native install proves the source remains owned.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    fn record_native_persist_failure(&mut self, error: &std::io::Error) {
        self.state = if LocalPersistFailureState::from_error(LocalPersistStage::InstallDestination, error.kind())
            == LocalPersistFailureState::NotPublished
        {
            LocalTempResourceState::Owned
        } else {
            LocalTempResourceState::Indeterminate
        };
    }
}

impl Drop for LocalTempDirectory {
    /// Attempts remaining tree or sandbox cleanup and discards cleanup errors.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Converts one authority-relative Rooted path into virtual absolute syntax.
fn virtual_rooted_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::from(std::path::MAIN_SEPARATOR_STR);
    result.push(path);
    result
}
