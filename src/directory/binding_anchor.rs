// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct BindingAnchor {
    /// Diagnostic path associated with the retained identity.
    pub(crate) path: PathBuf,
    /// Original namespace name used to reopen this identity during binding
    /// checks.
    pub(crate) opening_recipe: PathBuf,
    pub(crate) identity: crate::observation::LocalFileIdentity,
    pub(crate) authority: Arc<crate::rooted::Root>,
    pub(crate) contract: crate::resolution::ResolutionContract,
    pub(crate) symlink_policy: crate::LocalSymlinkPolicy,
}

impl std::fmt::Debug for BindingAnchor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BindingAnchor")
            .field("path", &self.path)
            .field("opening_recipe", &self.opening_recipe)
            .field("symlink_policy", &self.symlink_policy)
            .finish_non_exhaustive()
    }
}
