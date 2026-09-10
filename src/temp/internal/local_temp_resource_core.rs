// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared temporary-resource authority, state transitions, and diagnostics.
// qubit-style: allow source-test-pair
// Covered by public file and directory lifecycle integration tests.

use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Path;
use std::path::PathBuf;

use super::LocalTempResourceBackend;
use super::LocalTempResourceState;
use super::prepare_persist_target;
use super::validate_persist_base;
use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalPersistError;
use crate::LocalPersistFailureState;
use crate::LocalPersistStage;
use crate::LocalRelativePath;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::outcome::LocalTempSourceState;
use crate::path::LocalFileSystemScope;
use crate::path::LocalNamespacePath;
use crate::path::LocalPathResolver;

/// Authority and lifecycle shared by file and directory wrappers.
#[derive(Debug)]
pub(crate) struct LocalTempResourceCore {
    /// Namespace-absolute diagnostic source path; Host also uses native syntax.
    pub(crate) path: PathBuf,
    /// Bound native authority and exactly one matching identity.
    pub(crate) backend: LocalTempResourceBackend,
    /// Current namespace authority.
    pub(crate) state: LocalTempResourceState,
    /// Target symlink policy retained at construction.
    pub(crate) symlink_policy: LocalSymlinkPolicy,
    /// Creation-time PWD used only for error context.
    creation_current_directory: Option<PathBuf>,
}

impl LocalTempResourceCore {
    /// Takes a successfully bound backend and its namespace path.
    pub(crate) fn new(path: PathBuf, backend: LocalTempResourceBackend, symlink_policy: LocalSymlinkPolicy) -> Self {
        Self {
            path,
            backend,
            state: LocalTempResourceState::Owned,
            symlink_policy,
            creation_current_directory: None,
        }
    }

    /// Projects internal lifecycle state into the unique public source axis.
    pub(crate) const fn source_state(&self) -> LocalTempSourceState {
        match self.state {
            LocalTempResourceState::Owned => LocalTempSourceState::Owned,
            LocalTempResourceState::SandboxPending => LocalTempSourceState::CleanupRequired,
            LocalTempResourceState::Released => LocalTempSourceState::Released,
            LocalTempResourceState::Indeterminate => LocalTempSourceState::Indeterminate,
        }
    }

    /// Rejects publication unless the source remains owned, before target work.
    pub(crate) fn ensure_publishable(&self) -> Result<()> {
        if self.state != LocalTempResourceState::Owned {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "temporary resource does not own a publishable source",
            ));
        }
        Ok(())
    }

    /// Rejects automatic and explicit deletion when source authority is
    /// unknown.
    pub(crate) fn ensure_cleanup_safe(&self) -> Result<()> {
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "temporary resource source is indeterminate; cleanup is unsafe",
            ));
        }
        Ok(())
    }

    /// Rechecks the bound source identity without following a final link.
    /// A mismatch or failed inspection locks the source as indeterminate
    /// because authority could not be established. Native causes remain
    /// unchanged; a missing source is not successful cleanup.
    pub(crate) fn ensure_identity_matches(&mut self) -> Result<()> {
        self.ensure_publishable()?;
        let matches = match &self.backend {
            LocalTempResourceBackend::Host(host) => host.identity.matches_path(&self.path),
            LocalTempResourceBackend::Rooted(rooted) => rooted
                .root
                .symlink_metadata(
                    &LocalRelativePath::new(&rooted.relative_path).expect("temporary source was validated at creation"),
                )
                .map(|metadata| metadata.is_same_file(&rooted.identity)),
        };
        match matches {
            Ok(true) => Ok(()),
            Ok(false) => {
                self.state = LocalTempResourceState::Indeterminate;
                Err(Error::new(
                    ErrorKind::InvalidInput,
                    "temporary resource path no longer names the created entry",
                ))
            }
            Err(error) => {
                self.state = LocalTempResourceState::Indeterminate;
                Err(error)
            }
        }
    }

    /// Records an actual native rename failure and returns only its publication
    /// fact. A definite failure retains authority only after a fresh identity
    /// check; an uncertain rename permanently disables source operations.
    pub(crate) fn record_native_persist_failure(&mut self, error: &Error) -> LocalPersistFailureState {
        let publication = LocalPersistFailureState::from_native_error(error.kind());
        if publication == LocalPersistFailureState::Indeterminate || self.ensure_identity_matches().is_err() {
            self.state = LocalTempResourceState::Indeterminate;
        }
        publication
    }

    /// Removes an empty sandbox through the retained authority. Native errors
    /// retain sandbox cleanup responsibility in the caller.
    pub(crate) fn release_sandbox(&self) -> Result<()> {
        match &self.backend {
            LocalTempResourceBackend::Host(host) => std::fs::remove_dir(&host.sandbox_path),
            LocalTempResourceBackend::Rooted(rooted) => {
                let sandbox =
                    LocalRelativePath::new(&rooted.sandbox_path).expect("temporary sandbox was validated at creation");
                rooted.root.remove_empty_dir(&sandbox)
            }
        }
    }

    /// Returns the public sandbox path used solely for cleanup diagnostics.
    pub(crate) fn cleanup_path(&self) -> PathBuf {
        match &self.backend {
            LocalTempResourceBackend::Host(host) => host.sandbox_path.clone(),
            LocalTempResourceBackend::Rooted(rooted) => virtual_rooted_path(&rooted.sandbox_path),
        }
    }

    /// Adds creation-time PWD context to a native operation failure.
    pub(crate) fn contextualize_error(&self, error: LocalFileError) -> LocalFileError {
        match &self.creation_current_directory {
            Some(directory) => error.with_current_directory(directory.clone()),
            None => error,
        }
    }

    /// Captures both explicitly established publication and current source
    /// facts. The wrapper attaches itself after this borrow ends.
    pub(crate) fn persist_error(
        &self,
        error: Error,
        requested_target: PathBuf,
        resolved_target: Option<PathBuf>,
        stage: LocalPersistStage,
        publication: LocalPersistFailureState,
    ) -> LocalPersistError<()> {
        let error = LocalPersistError::new(
            error,
            (),
            requested_target,
            resolved_target,
            stage,
            publication,
            self.source_state(),
        );
        match &self.creation_current_directory {
            Some(directory) => error.with_current_directory(directory.clone()),
            None => error,
        }
    }

    /// Resolves the public source and retains PWD diagnostics. Resolution
    /// errors identify the creating operation; authority remains the
    /// original backend.
    pub(crate) fn bind_namespace(
        &mut self,
        resolver: LocalPathResolver,
        operation: LocalFileOperation,
    ) -> LocalResult<()> {
        let input = match &self.backend {
            LocalTempResourceBackend::Host(_) => self.path.clone(),
            LocalTempResourceBackend::Rooted(rooted) => virtual_rooted_path(&rooted.relative_path),
        };
        self.path = resolver
            .resolve(&input)
            .map_err(|error| {
                let error = error.with_operation(operation);
                match resolver.current_directory() {
                    Some(directory) => error.with_current_directory(directory.to_path_buf()),
                    None => error,
                }
            })?
            .namespace_absolute()
            .to_path_buf();
        self.creation_current_directory = resolver.current_directory().map(Path::to_path_buf);
        Ok(())
    }

    /// Returns the namespace scope carried by the bound authority.
    pub(crate) const fn scope(&self) -> LocalFileSystemScope {
        match &self.backend {
            LocalTempResourceBackend::Host(_) => LocalFileSystemScope::Host,
            LocalTempResourceBackend::Rooted(_) => LocalFileSystemScope::Rooted,
        }
    }

    /// Performs lexical target preparation without changing source authority.
    pub(crate) fn prepare_target(&self, base: Option<&Path>, target: &Path) -> LocalResult<LocalNamespacePath> {
        prepare_persist_target(self.scope(), base, target)
    }

    /// Checks an explicit base through the retained authority and symlink
    /// policy. Native lookup and invalid-directory failures retain their
    /// path context.
    pub(crate) fn validate_base(&self, base: &Path) -> LocalResult<()> {
        validate_persist_base(&self.backend, base, self.symlink_policy)
    }
}

/// Converts an authority-relative path into public virtual absolute syntax.
fn virtual_rooted_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::from(std::path::MAIN_SEPARATOR_STR);
    result.push(path);
    result
}
