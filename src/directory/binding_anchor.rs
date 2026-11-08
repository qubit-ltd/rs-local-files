// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fmt::Debug;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::path::PathBuf;
use std::sync::Arc;

/// Retains the identity and reopening contract for a directory binding.
///
/// The anchor keeps both the diagnostic path and the original opening recipe
/// so a later binding check can resolve the same authority under the original
/// resolution limits and symlink policy. Its debug output redacts both paths.
#[derive(Clone)]
pub(crate) struct BindingAnchor {
    /// Diagnostic path associated with the retained identity.
    pub(crate) path: PathBuf,
    /// Original namespace name used to reopen this identity during binding
    /// checks.
    pub(crate) opening_recipe: PathBuf,
    /// Native identity to compare when the opening name is re-resolved.
    pub(crate) identity: crate::observation::LocalFileIdentity,
    /// Retained authority from which the original binding was derived.
    pub(crate) authority: Arc<crate::rooted::Root>,
    /// Resolution limits and validation hooks to preserve on recheck.
    pub(crate) contract: crate::resolution::ResolutionContract,
    /// Symlink behavior captured when the authority was opened.
    pub(crate) symlink_policy: crate::LocalSymlinkPolicy,
}

impl Debug for BindingAnchor {
    /// Formats the retained binding details while redacting path information.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Receives the debug representation without either path.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        formatter
            .debug_struct("BindingAnchor")
            .field("path", &crate::local_file_system::redacted_path(&self.path))
            .field(
                "opening_recipe",
                &crate::local_file_system::redacted_path(&self.opening_recipe),
            )
            .field("symlink_policy", &self.symlink_policy)
            .finish_non_exhaustive()
    }
}
