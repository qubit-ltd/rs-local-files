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
use std::time::Instant;

use super::internal::LocalTempResourceBackend;
use super::internal::LocalTempResourceCore;
use super::internal::LocalTempResourceState;
use super::internal::RootedTempResourceBackend;
use super::internal::TempDirectoryDeleteBackend;
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
use crate::local::DeleteBudget;
use crate::local::remove_directory_tree;
use crate::options::LocalTempCleanupLimits;
use crate::outcome::LocalTempSourceState;
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
    /// Bound authority and shared source lifecycle.
    core: LocalTempResourceCore,
    /// Limits reused by explicit cleanup and Drop, with a fresh budget per
    /// call.
    cleanup_limits: LocalTempCleanupLimits,
}

impl LocalTempDirectory {
    /// Builds a host temporary directory from its already-bound path.
    /// Captures identity without following the final link. Native inspection
    /// failure leaves cleanup of the created directory and sandbox to the
    /// caller.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub(crate) fn host(
        path: PathBuf,
        sandbox_path: PathBuf,
        symlink_policy: LocalSymlinkPolicy,
        cleanup_limits: LocalTempCleanupLimits,
    ) -> Result<Self> {
        let identity = TempEntryIdentity::from_path(&path)?;
        Ok(Self {
            cleanup_limits,
            core: LocalTempResourceCore::new(
                path,
                LocalTempResourceBackend::Host(super::internal::HostTempResourceBackend { sandbox_path, identity }),
                symlink_policy,
            ),
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
        cleanup_limits: LocalTempCleanupLimits,
    ) -> Result<Self> {
        let relative = LocalRelativePath::new(&path).expect("rooted temporary path was validated at creation");
        let identity = root.symlink_metadata(&relative)?;
        Ok(Self {
            cleanup_limits,
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
        })
    }

    /// Returns the namespace-absolute generated path.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn path(&self) -> &Path {
        &self.core.path
    }

    /// Returns the current source authority, independently of earlier
    /// publication.
    #[must_use = "inspect the source authority before choosing a recovery action"]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn source_state(&self) -> LocalTempSourceState {
        self.core.source_state()
    }

    /// Returns the limits retained for each explicit or Drop cleanup attempt.
    #[must_use = "inspect the limits retained for explicit and automatic cleanup"]
    pub const fn cleanup_limits(&self) -> LocalTempCleanupLimits {
        self.cleanup_limits
    }

    /// Replaces the retained cleanup limits without performing I/O.
    /// The next cleanup or Drop starts a fresh budget using `limits`.
    pub fn set_cleanup_limits(&mut self, limits: LocalTempCleanupLimits) {
        self.cleanup_limits = limits;
    }

    /// Removes the directory tree through the retained authority.
    ///
    /// Returns a structured error when identity inspection, tree removal, or
    /// empty-sandbox removal fails, or namespace certainty makes cleanup
    /// unsafe. Tree removal is incremental and cannot be rolled back. A
    /// sandbox-only failure can be retried; successful cleanup is idempotent.
    /// Each attempt uses the retained limits and a fresh budget. Drop makes at
    /// most one further attempt with those same limits. The sandbox is outside
    /// source entry/path accounting but shares this attempt's deadline. Checks
    /// are cooperative; an in-flight native call is not interrupted. Children
    /// are not sorted, and queued paths still require memory. Concurrent new
    /// children cause an error without an automatic rescan.
    pub fn cleanup(&mut self) -> LocalResult<()> {
        let started_at = Instant::now();
        self.ensure_cleanup_safe().map_err(|error| {
            self.contextualize_error(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.core.path.clone()),
                None,
                error,
            ))
        })?;
        let removed_source = self.core.state == LocalTempResourceState::Owned;
        if removed_source {
            self.remove_resource(started_at)?;
            self.core.state = LocalTempResourceState::SandboxPending;
        }
        if self.core.state == LocalTempResourceState::SandboxPending {
            DeleteBudget::new(self.cleanup_limits.delete_options(), started_at)
                .check_deadline()
                .map_err(|error| {
                    self.contextualize_error(crate::local::directory_mutation_error(
                        LocalFileOperation::Cleanup,
                        &self.cleanup_path(),
                        removed_source,
                        error,
                    ))
                })?;
            self.release_sandbox().map_err(|error| {
                self.contextualize_error(crate::local::directory_mutation_error(
                    LocalFileOperation::Cleanup,
                    &self.cleanup_path(),
                    removed_source,
                    error,
                ))
            })?;
            self.core.state = LocalTempResourceState::Released;
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
        Ok(self.core.path.join(relative.as_path()))
    }

    /// Resolves a normal relative descendant below this directory.
    /// Performs no I/O or existence check. Returns a relative-path validation
    /// error for empty input, roots, prefixes, dots, parents, or native NUL.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn descendant(&self, descendant: &Path) -> Result<PathBuf> {
        let relative = LocalRelativePath::new(descendant)?;
        Ok(self.core.path.join(relative.as_path()))
    }

    /// Atomically publishes the directory to a generated sibling outside its
    /// private sandbox.
    /// Uses the default no-replacement policy of [`Self::persist_with`], with
    /// the same publication outcome and resource-retaining failure contract.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
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
        self.core
            .bind_namespace(resolver, LocalFileOperation::CreateTempDirectory)?;
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

        if options.durability() == LocalDurabilityRequirement::Required {
            return Err(self
                .persist_error(
                    Error::new(
                        ErrorKind::Unsupported,
                        "required temporary-directory content durability cannot be guaranteed",
                    ),
                    requested_target,
                    Some(namespace_target),
                    LocalPersistStage::SynchronizeSource,
                    LocalPersistFailureState::NotPublished,
                )
                .with_kind(LocalFileErrorKind::RequirementNotMet));
        }
        let authority_target = resolved_target.authority_relative().to_path_buf();
        match &self.core.backend {
            LocalTempResourceBackend::Host(_) => {
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
                if let Err(error) = prepare_host_parent(&target, options.creates_parent()) {
                    return Err(self.persist_error(
                        error,
                        requested_target,
                        Some(namespace_target),
                        LocalPersistStage::PrepareParent,
                        LocalPersistFailureState::NotPublished,
                    ));
                }
                let result = if options.overwrites() {
                    std::fs::rename(&self.core.path, &target)
                } else {
                    crate::local::move_directory_without_replacing(&self.core.path, &target)
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
                            LocalPersistFailureState::NotPublished,
                        ));
                    }
                };
                let source = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
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
    fn remove_resource(&mut self, started_at: Instant) -> LocalResult<()> {
        self.ensure_identity_matches().map_err(|error| {
            self.contextualize_error(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.core.path.clone()),
                None,
                error,
            ))
        })?;
        let options = self.cleanup_limits.delete_options();
        let result = match &self.core.backend {
            LocalTempResourceBackend::Host(host) => remove_directory_tree(host, &self.core.path, options, started_at),
            LocalTempResourceBackend::Rooted(rooted) => {
                let path = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                remove_directory_tree(
                    &TempDirectoryDeleteBackend { root: &rooted.root },
                    &path,
                    options,
                    started_at,
                )
                .map_err(|error| {
                    let path = error
                        .path()
                        .map(|path| Path::new(std::path::MAIN_SEPARATOR_STR).join(path));
                    match path {
                        Some(path) => error.with_path(path),
                        None => error,
                    }
                })
            }
        };
        result.map_err(|error| self.contextualize_error(error.with_operation(LocalFileOperation::Cleanup)))
    }

    /// Removes the now-empty private sandbox.
    /// Propagates native removal errors, including a non-empty sandbox.
    fn release_sandbox(&self) -> Result<()> {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("temp-directory-sandbox-remove") {
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
        let error = self
            .core
            .persist_error(error, requested_target, resolved_target, stage, publication);
        error.with_resource(self)
    }

    /// Attaches the resource's creation-time PWD to a structured error.
    fn contextualize_error(&self, error: LocalFileError) -> LocalFileError {
        self.core.contextualize_error(error)
    }

    /// Rejects namespace cleanup after an indeterminate native publication
    /// attempt.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    fn ensure_cleanup_safe(&self) -> Result<()> {
        self.core.ensure_cleanup_safe()
    }

    /// Rejects operations when the authority path no longer names this
    /// directory.
    /// A mismatch marks the state indeterminate and returns `InvalidInput`;
    /// failed identity inspection also marks the state indeterminate while
    /// preserving its native error.
    fn ensure_identity_matches(&mut self) -> Result<()> {
        self.core.ensure_identity_matches()
    }

    /// Records whether a failed native install proves the source remains owned.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    fn record_native_persist_failure(&mut self, error: &Error) -> LocalPersistFailureState {
        self.core.record_native_persist_failure(error)
    }
}

impl Drop for LocalTempDirectory {
    /// Attempts remaining tree or sandbox cleanup and discards cleanup errors.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
