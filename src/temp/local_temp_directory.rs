// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup-owned temporary directories with host or rooted authority.

use std::{
    io::Result,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use log::warn;

use crate::{
    LocalPersistError,
    LocalPersistFailureState,
    LocalPersistMethod,
    LocalPersistOptions,
    LocalPersistOutcome,
    LocalPersistStage,
    LocalRelativePath,
};

use super::internal::{
    LocalTempResourceBackend,
    LocalTempResourceState,
    RootedTempResourceBackend,
};

/// A temporary directory whose cleanup remains bound to its creating authority.
#[must_use = "dropping the temporary-directory guard removes its directory"]
#[derive(Debug)]
pub struct LocalTempDirectory {
    /// Stable authority-local generated path.
    path: PathBuf,
    /// Authority and resource ownership state.
    backend: LocalTempResourceBackend,
    /// Namespace certainty governing cleanup and drop behavior.
    state: LocalTempResourceState,
}

impl LocalTempDirectory {
    /// Builds a host temporary directory from its already-bound path.
    pub(crate) fn host(path: PathBuf) -> Self {
        Self {
            path,
            backend: LocalTempResourceBackend::Host(
                super::internal::HostTempResourceBackend,
            ),
            state: LocalTempResourceState::Owned,
        }
    }

    /// Builds a rooted temporary directory from the retained root authority.
    pub(crate) fn rooted(
        root: Arc<crate::rooted::Root>,
        path: PathBuf,
    ) -> Self {
        Self {
            path: path.clone(),
            backend: LocalTempResourceBackend::Rooted(
                RootedTempResourceBackend {
                    root,
                    relative_path: path,
                },
            ),
            state: LocalTempResourceState::Owned,
        }
    }

    /// Returns the authority-local generated path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Removes the directory tree through the retained authority.
    pub fn cleanup(&mut self) -> Result<()> {
        self.ensure_cleanup_safe()?;
        self.remove()?;
        self.state = LocalTempResourceState::Released;
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
    pub fn descendant(&self, descendant: &Path) -> Result<PathBuf> {
        let relative = LocalRelativePath::new(descendant)?;
        Ok(self.path.join(relative.as_path()))
    }

    /// Disables cleanup and returns the authority-local directory path.
    #[must_use = "keeping the temporary directory disables automatic cleanup; retain the returned path"]
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
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>> {
        self.persist_with(target, LocalPersistOptions::new())
    }

    /// Persists the directory with an explicit replacement policy through its
    /// creating authority.
    #[inline(always)]
    pub fn persist_with(
        self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>> {
        self.persist_with_outcome(target, options)
            .map(LocalPersistOutcome::into_path)
    }

    /// Persists the directory and reports the achieved publication guarantees.
    #[inline(always)]
    pub fn persist_with_outcome(
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
                let target = match std::path::absolute(&requested_target) {
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
                if let Err(error) = crate::local::ensure_parent_path(&target) {
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
                self.state = LocalTempResourceState::Released;
                Ok(LocalPersistOutcome::new(
                    target,
                    LocalPersistMethod::AtomicRename,
                    true,
                    false,
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
                let destination = LocalRelativePath::new(&target)
                    .expect("persist target was validated");
                let result = if options.overwrites() {
                    rooted.root.rename(&source, &destination)
                } else {
                    rooted.root.rename_without_replacing(&source, &destination)
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
                self.state = LocalTempResourceState::Released;
                Ok(LocalPersistOutcome::new(
                    target,
                    LocalPersistMethod::AtomicRename,
                    true,
                    false,
                ))
            }
        }
    }

    /// Removes the resource using the retained backend rather than a diagnostic
    /// path.
    fn remove(&self) -> Result<()> {
        match &self.backend {
            LocalTempResourceBackend::Host(_) => {
                std::fs::remove_dir_all(&self.path)
            }
            LocalTempResourceBackend::Rooted(rooted) => {
                let path = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                rooted.root.remove_tree(&path)
            }
        }
    }

    /// Rejects namespace cleanup after an indeterminate native publication
    /// attempt.
    fn ensure_cleanup_safe(&self) -> Result<()> {
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(std::io::Error::other(
                "temporary directory namespace state is indeterminate; cleanup is unsafe",
            ));
        }
        Ok(())
    }

    /// Records whether a failed native install proves the source remains owned.
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
        if matches!(self.state, LocalTempResourceState::Owned)
            && let Err(error) = self.remove()
        {
            warn!(
                "failed to remove temporary directory {}: {}",
                self.path.display(),
                error
            );
        }
    }
}
