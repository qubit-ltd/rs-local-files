// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup-owned temporary directories with host or rooted authority.

use std::{
    io::{
        Error,
        ErrorKind,
        Result,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use log::warn;

use crate::{
    LocalFileError,
    LocalFileOperation,
    LocalPersistError,
    LocalPersistFailureState,
    LocalPersistMethod,
    LocalPersistOptions,
    LocalPersistOutcome,
    LocalPersistStage,
    LocalRelativePath,
    LocalResult,
    LocalSymlinkPolicy,
};

use super::internal::{
    LocalTempResourceBackend,
    LocalTempResourceState,
    RootedTempResourceBackend,
    TempEntryIdentity,
    prepare_host_parent,
    prepare_rooted_parent,
};

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
/// removes the directory tree and then the empty sandbox; [`Self::keep`]
/// transfers both to the caller.
#[must_use = "dropping the temporary-directory guard removes its directory"]
#[derive(Debug)]
pub struct LocalTempDirectory {
    /// Stable authority-local generated path.
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
}

impl LocalTempDirectory {
    /// Builds a host temporary directory from its already-bound path.
    #[inline]
    pub(crate) fn host(
        path: PathBuf,
        sandbox_path: PathBuf,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        Ok(Self {
            host_identity: Some(TempEntryIdentity::from_path(&path)?),
            rooted_identity: None,
            path,
            backend: LocalTempResourceBackend::Host(
                super::internal::HostTempResourceBackend { sandbox_path },
            ),
            state: LocalTempResourceState::Owned,
            symlink_policy,
        })
    }

    /// Builds a rooted temporary directory from the retained root authority.
    #[inline]
    pub(crate) fn rooted(
        root: Arc<crate::rooted::Root>,
        path: PathBuf,
        sandbox_path: PathBuf,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        let relative = LocalRelativePath::new(&path)
            .expect("rooted temporary path was validated at creation");
        Ok(Self {
            host_identity: None,
            rooted_identity: Some(root.symlink_metadata(&relative)?),
            path: path.clone(),
            backend: LocalTempResourceBackend::Rooted(
                RootedTempResourceBackend {
                    root,
                    relative_path: path,
                    sandbox_path,
                },
            ),
            state: LocalTempResourceState::Owned,
            symlink_policy,
        })
    }

    /// Returns the authority-local generated path.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Removes the directory tree through the retained authority.
    pub fn cleanup(&mut self) -> LocalResult<()> {
        self.ensure_cleanup_safe().map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.path.clone()),
                None,
                error,
            )
        })?;
        if self.state == LocalTempResourceState::Owned {
            self.remove_resource().map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.path.clone()),
                    None,
                    error,
                )
            })?;
            self.state = LocalTempResourceState::SandboxPending;
        }
        if self.state == LocalTempResourceState::SandboxPending {
            self.release_sandbox().map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.cleanup_path()),
                    None,
                    error,
                )
            })?;
            self.state = LocalTempResourceState::Released;
        }
        Ok(())
    }

    /// Resolves one normal child component below this directory.
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
    #[inline]
    pub fn descendant(&self, descendant: &Path) -> Result<PathBuf> {
        let relative = LocalRelativePath::new(descendant)?;
        Ok(self.path.join(relative.as_path()))
    }

    /// Disables cleanup and returns the authority-local directory path.
    #[must_use = "keeping the temporary directory disables automatic cleanup; retain the returned path"]
    #[inline]
    pub fn keep(mut self) -> PathBuf {
        self.state = LocalTempResourceState::Released;
        self.path.clone()
    }

    /// Persists the directory without replacement through its creating
    /// authority.
    #[inline(always)]
    pub fn persist(
        self,
        target: impl AsRef<Path>,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with(target, LocalPersistOptions::new())
    }

    /// Persists the directory with an explicit replacement policy through its
    /// creating authority.
    #[inline(always)]
    pub fn persist_with(
        self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path(target.as_ref(), options)
    }

    /// Persists the directory to a resolved public-API target path.
    fn persist_with_path(
        mut self,
        target: &Path,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        if let Err(error) = self.ensure_identity_matches() {
            return Err(LocalPersistError::new(
                error,
                self,
                target.to_path_buf(),
                None,
                LocalPersistStage::InstallDestination,
            ));
        }
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(LocalPersistError::new(
                std::io::Error::other(
                    "temporary directory namespace state is indeterminate",
                ),
                self,
                target.to_path_buf(),
                None,
                LocalPersistStage::InstallDestination,
            ));
        }
        let requested_target = target.to_path_buf();
        match &self.backend {
            LocalTempResourceBackend::Host(_) => {
                let logical_target =
                    match std::path::absolute(&requested_target) {
                        Ok(target) => target,
                        Err(error) => {
                            return Err(LocalPersistError::new(
                                error,
                                self,
                                requested_target,
                                None,
                                LocalPersistStage::ResolveTarget,
                            ));
                        }
                    };
                let target = match crate::local::resolve_host_path(
                    &logical_target,
                    self.symlink_policy,
                    false,
                ) {
                    Ok(target) => target,
                    Err(error) => {
                        return Err(LocalPersistError::new(
                            error.into_io_error(),
                            self,
                            requested_target,
                            None,
                            LocalPersistStage::ResolveTarget,
                        ));
                    }
                };
                if let Err(error) =
                    prepare_host_parent(&target, options.creates_parent())
                {
                    return Err(LocalPersistError::new(
                        error,
                        self,
                        requested_target,
                        Some(target),
                        LocalPersistStage::PrepareParent,
                    ));
                }
                let result = if options.overwrites() {
                    std::fs::rename(&self.path, &target)
                } else {
                    crate::local::move_directory_without_replacing(
                        &self.path, &target,
                    )
                };
                if let Err(error) = result {
                    self.record_native_persist_failure(&error);
                    return Err(LocalPersistError::new(
                        error,
                        self,
                        requested_target,
                        Some(target),
                        LocalPersistStage::InstallDestination,
                    ));
                }
                let cleanup_error = self.release_sandbox().err().map(|error| {
                    LocalFileError::from_io(
                        LocalFileOperation::Cleanup,
                        Some(self.cleanup_path()),
                        None,
                        error,
                    )
                });
                self.state = LocalTempResourceState::Released;
                Ok(LocalPersistOutcome::new(
                    logical_target,
                    LocalPersistMethod::AtomicRename,
                    true,
                    false,
                    cleanup_error,
                ))
            }
            LocalTempResourceBackend::Rooted(rooted) => {
                let target = match LocalRelativePath::new(&requested_target) {
                    Ok(target) => target.as_path().to_path_buf(),
                    Err(error) => {
                        return Err(LocalPersistError::new(
                            error,
                            self,
                            requested_target,
                            None,
                            LocalPersistStage::ResolveTarget,
                        ));
                    }
                };
                let source = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                let resolved =
                    match crate::rooted_local_file_system::resolve_rooted_path(
                        &rooted.root,
                        &target,
                        self.symlink_policy,
                        false,
                        LocalFileOperation::PersistTemp,
                    ) {
                        Ok(resolved) => resolved,
                        Err(error) => {
                            return Err(LocalPersistError::new(
                                error.into_io_error(),
                                self,
                                requested_target,
                                None,
                                LocalPersistStage::ResolveTarget,
                            ));
                        }
                    };
                let destination = resolved;
                if let Err(error) = prepare_rooted_parent(
                    &rooted.root,
                    &destination,
                    options.creates_parent(),
                ) {
                    return Err(LocalPersistError::new(
                        error,
                        self,
                        requested_target,
                        Some(destination.as_path().to_path_buf()),
                        LocalPersistStage::PrepareParent,
                    ));
                }
                let result = if options.overwrites() {
                    rooted.root.rename(&source, &destination)
                } else {
                    rooted.root.rename_without_replacing(&source, &destination)
                };
                let target = destination.as_path().to_path_buf();
                if let Err(error) = result {
                    self.record_native_persist_failure(&error);
                    return Err(LocalPersistError::new(
                        error,
                        self,
                        requested_target,
                        Some(target),
                        LocalPersistStage::InstallDestination,
                    ));
                }
                let cleanup_error = self.release_sandbox().err().map(|error| {
                    LocalFileError::from_io(
                        LocalFileOperation::Cleanup,
                        Some(self.cleanup_path()),
                        None,
                        error,
                    )
                });
                self.state = LocalTempResourceState::Released;
                Ok(LocalPersistOutcome::new(
                    target,
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
    #[inline]
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
    fn release_sandbox(&self) -> Result<()> {
        #[cfg(feature = "internal-test-support")]
        if crate::local::take_test_support("temp-directory-sandbox-remove") {
            return Err(crate::local::test_fault_error());
        }
        match &self.backend {
            LocalTempResourceBackend::Host(host) => {
                std::fs::remove_dir(&host.sandbox_path)
            }
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
            LocalTempResourceBackend::Rooted(rooted) => {
                rooted.sandbox_path.clone()
            }
        }
    }

    /// Rejects namespace cleanup after an indeterminate native publication
    /// attempt.
    #[inline]
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
    #[inline]
    fn record_native_persist_failure(&mut self, error: &std::io::Error) {
        self.state = if LocalPersistFailureState::from_error(
            LocalPersistStage::InstallDestination,
            error.kind(),
        ) == LocalPersistFailureState::NotPublished
        {
            LocalTempResourceState::Owned
        } else {
            LocalTempResourceState::Indeterminate
        };
    }
}

impl Drop for LocalTempDirectory {
    /// Performs best-effort cleanup only while the directory remains owned.
    fn drop(&mut self) {
        match self.state {
            LocalTempResourceState::Owned => {
                if let Err(error) = self.remove_resource() {
                    // TODO: Route cleanup diagnostics through caller-controlled
                    // redaction, sampling, and metrics policy.
                    warn!(
                        "failed to remove temporary directory {}: {}",
                        self.path.display(),
                        error
                    );
                    return;
                }
                self.state = LocalTempResourceState::SandboxPending;
                if let Err(error) = self.release_sandbox() {
                    // TODO: Route cleanup diagnostics through caller-controlled
                    // redaction, sampling, and metrics policy.
                    warn!(
                        "failed to remove temporary directory sandbox for {}: {}",
                        self.path.display(),
                        error
                    );
                } else {
                    self.state = LocalTempResourceState::Released;
                }
            }
            LocalTempResourceState::SandboxPending => {
                if let Err(error) = self.release_sandbox() {
                    // TODO: Route cleanup diagnostics through caller-controlled
                    // redaction, sampling, and metrics policy.
                    warn!(
                        "failed to remove temporary directory sandbox for {}: {}",
                        self.path.display(),
                        error
                    );
                } else {
                    self.state = LocalTempResourceState::Released;
                }
            }
            LocalTempResourceState::Indeterminate
            | LocalTempResourceState::Released => {}
        }
    }
}
