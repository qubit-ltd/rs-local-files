// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::local_tree_entry::LocalTreeEntry;
use crate::options::LocalListOptions;
use crate::policy::LocalOwnerAccessPolicy;
use crate::policy::LocalPrivacyRequirement;

/// Constraints applied while validating a staged tree.
///
/// An omitted exact-entry set does not request an exact-set check during
/// publication. Passing an explicit expected slice to `verify_entries` still
/// checks both missing and unexpected entries. Traversal limits bound observed
/// paths and entries; they are not a total memory limit.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use qubit_local_files::LocalFileKind;
/// use qubit_local_files::directory::{LocalTreeEntry, LocalTreeValidationOptions};
/// use qubit_local_files::options::LocalListOptions;
///
/// # fn main() -> Result<(), qubit_local_files::LocalFileError> {
/// let expected = LocalTreeEntry::new(PathBuf::from("payload"), LocalFileKind::File)?;
/// let options = LocalTreeValidationOptions::new()
///     .with_exact_entries(vec![expected])
///     .with_limits(LocalListOptions::new().with_max_entries(16));
/// let _ = options;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct LocalTreeValidationOptions {
    /// Exact expected entries, when configured.
    exact_entries: Option<Vec<LocalTreeEntry>>,
    /// Whether symbolic links invalidate validation.
    reject_symlinks: bool,
    /// Whether FIFO, socket, device, and unknown kinds invalidate validation.
    reject_special_files: bool,
    /// Whether files with hard links outside the checked tree invalidate validation.
    reject_external_hardlinks: bool,
    /// Required privacy level for entries when supported.
    privacy: LocalPrivacyRequirement,
    /// Exact owner access policy, if one is required.
    owner_access_policy: Option<LocalOwnerAccessPolicy>,
    /// Traversal limits shared by complete-tree validation.
    limits: LocalListOptions,
}

impl LocalTreeValidationOptions {
    /// Creates validation options that check only the stage authority and
    /// identity during publication. Passing these defaults to `verify_entries`
    /// still performs a complete exact-set check, because that method receives
    /// an explicit expected slice.
    pub const fn new() -> Self {
        Self {
            exact_entries: None,
            reject_symlinks: false,
            reject_special_files: false,
            reject_external_hardlinks: false,
            privacy: LocalPrivacyRequirement::PlatformDefault,
            owner_access_policy: None,
            limits: LocalListOptions::new(),
        }
    }
    /// Requires the observed tree to contain exactly `entries`.
    ///
    /// Duplicate paths, including the same path with different kinds, make
    /// verification invalid and return `InvalidOptions`. The paths are indexed
    /// by borrowed references; their encoded bytes count toward the logical
    /// path budget before indexing begins.
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
    /// Requires every entry to match an exact owner access policy.
    pub const fn with_owner_access_policy(mut self, policy: LocalOwnerAccessPolicy) -> Self {
        self.owner_access_policy = Some(policy);
        self
    }
    /// Applies traversal resource limits to complete-tree validation.
    ///
    /// Finite depth is a complete-validation ceiling: empty trees and entries
    /// at the boundary pass, while an observed entry below the boundary fails
    /// with `ResourceLimit`. The seen-name byte limit counts expected encoded
    /// paths first and then observed encoded paths; it is not a total heap
    /// memory limit.
    pub const fn with_limits(mut self, limits: LocalListOptions) -> Self {
        self.limits = limits;
        self
    }
    /// Returns the configured exact set; `None` means publication did not
    /// receive a stored exact set through these options.
    pub(crate) fn exact_entries(&self) -> Option<&[LocalTreeEntry]> {
        self.exact_entries.as_deref()
    }
    /// Reports whether symbolic links are excluded.
    pub(crate) const fn rejects_symlinks(&self) -> bool {
        self.reject_symlinks
    }
    /// Reports whether special and unknown native kinds are excluded.
    pub(crate) const fn rejects_special_files(&self) -> bool {
        self.reject_special_files
    }
    /// Reports whether external hard links are excluded.
    pub(crate) const fn rejects_external_hardlinks(&self) -> bool {
        self.reject_external_hardlinks
    }
    /// Returns the requested privacy level.
    pub(crate) const fn privacy(&self) -> LocalPrivacyRequirement {
        self.privacy
    }
    /// Returns the owner policy, or `None` when no exact policy was requested.
    pub(crate) const fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
    /// Returns the traversal budget applied to complete-tree validation.
    pub(crate) const fn limits(&self) -> &LocalListOptions {
        &self.limits
    }
}

impl Default for LocalTreeValidationOptions {
    fn default() -> Self {
        Self::new()
    }
}
