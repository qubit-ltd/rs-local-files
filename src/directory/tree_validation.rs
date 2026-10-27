// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::observation::LocalFileObservation;
use crate::options::LocalListOptions;
use crate::policy::LocalPrivacyRequirement;

/// One expected entry in a staged directory tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTreeEntry {
    relative_path: PathBuf,
    kind: LocalFileKind,
}

impl LocalTreeEntry {
    /// Creates an expected entry from a safe relative path and an ordinary tree
    /// kind.
    ///
    /// Returns `InvalidPath` for empty, absolute, dotted, parent, or prefixed
    /// paths and `InvalidOptions` for kinds other than file, directory, and
    /// symbolic link.
    pub fn new(relative_path: PathBuf, kind: LocalFileKind) -> LocalResult<Self> {
        let relative_path = crate::LocalRelativePath::new(&relative_path)
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::ValidateName,
                    Some(relative_path.clone()),
                    None,
                    source,
                )
            })?
            .as_path()
            .to_path_buf();
        let valid_kind = matches!(
            kind,
            LocalFileKind::File | LocalFileKind::Directory | LocalFileKind::Symlink
        );
        if !valid_kind {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::ValidateName,
            ));
        }
        Ok(Self { relative_path, kind })
    }

    /// Returns this entry's path relative to the staged directory.
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns the expected native entry kind.
    pub const fn kind(&self) -> LocalFileKind {
        self.kind
    }
}

/// Observation of one entry yielded from a staged tree.
#[derive(Clone, Debug)]
pub struct LocalTreeEntryObservation {
    relative_path: PathBuf,
    observation: LocalFileObservation,
}

impl LocalTreeEntryObservation {
    pub(crate) const fn new(relative_path: PathBuf, observation: LocalFileObservation) -> Self {
        Self {
            relative_path,
            observation,
        }
    }

    /// Returns this entry's path relative to the staged directory.
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns the native observation captured for this entry.
    #[must_use]
    pub const fn observation(&self) -> &LocalFileObservation {
        &self.observation
    }
}

/// Optional constraints applied while validating a staged tree.
#[derive(Clone, Debug)]
pub struct LocalTreeValidationOptions {
    exact_entries: Option<Vec<LocalTreeEntry>>,
    reject_symlinks: bool,
    reject_special_files: bool,
    reject_external_hardlinks: bool,
    privacy: LocalPrivacyRequirement,
    limits: LocalListOptions,
}

impl LocalTreeValidationOptions {
    /// Creates validation options that check only the stage authority and
    /// identity.
    pub const fn new() -> Self {
        Self {
            exact_entries: None,
            reject_symlinks: false,
            reject_special_files: false,
            reject_external_hardlinks: false,
            privacy: LocalPrivacyRequirement::PlatformDefault,
            limits: LocalListOptions::new(),
        }
    }
    /// Requires the observed tree to contain exactly `entries`.
    pub fn with_exact_entries(mut self, entries: Vec<LocalTreeEntry>) -> Self {
        self.exact_entries = Some(entries);
        self
    }
    /// Rejects symbolic-link entries during validation.
    pub const fn with_reject_symlinks(mut self) -> Self {
        self.reject_symlinks = true;
        self
    }
    /// Rejects FIFO, socket, block, character, and unknown entries.
    pub const fn with_reject_special_files(mut self) -> Self {
        self.reject_special_files = true;
        self
    }
    /// Rejects files with hardlinks outside the checked tree.
    pub const fn with_reject_external_hardlinks(mut self) -> Self {
        self.reject_external_hardlinks = true;
        self
    }
    /// Requires current-user-only access where the platform can verify it.
    pub const fn with_privacy(mut self, privacy: LocalPrivacyRequirement) -> Self {
        self.privacy = privacy;
        self
    }
    /// Applies traversal resource limits to complete-tree validation.
    pub const fn with_limits(mut self, limits: LocalListOptions) -> Self {
        self.limits = limits;
        self
    }
    pub(crate) fn exact_entries(&self) -> Option<&[LocalTreeEntry]> {
        self.exact_entries.as_deref()
    }
    pub(crate) const fn rejects_symlinks(&self) -> bool {
        self.reject_symlinks
    }
    pub(crate) const fn rejects_special_files(&self) -> bool {
        self.reject_special_files
    }
    pub(crate) const fn rejects_external_hardlinks(&self) -> bool {
        self.reject_external_hardlinks
    }
    pub(crate) const fn privacy(&self) -> LocalPrivacyRequirement {
        self.privacy
    }
    pub(crate) const fn limits(&self) -> &LocalListOptions {
        &self.limits
    }
}

impl Default for LocalTreeValidationOptions {
    fn default() -> Self {
        Self::new()
    }
}
