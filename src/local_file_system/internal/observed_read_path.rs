// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use crate::directory::LocalComponentPolicy;

/// Selects how the observed-read core interprets its input path.
pub(crate) enum ObservedReadPath<'a> {
    /// A path in the filesystem namespace, resolved using its normal PWD rules.
    Namespace(&'a Path),
    /// An unmodified recipe relative to an already retained directory
    /// authority.
    FixedRelative {
        /// Caller-supplied relative path, including any links and parent steps.
        original: &'a Path,
        /// Diagnostic namespace coordinate from the fixed handle and recipe.
        request_path: &'a Path,
        /// Component restrictions inherited by the fixed directory handle.
        component_policy: &'a LocalComponentPolicy,
    },
}

impl ObservedReadPath<'_> {
    /// Returns the exact path supplied to the selected interpretation mode.
    pub(crate) fn original(&self) -> &Path {
        match self {
            Self::Namespace(path) => path,
            Self::FixedRelative { original, .. } => original,
        }
    }

    /// Returns the namespace coordinate used to attach observed-read errors.
    pub(crate) fn request_path(&self) -> &Path {
        match self {
            Self::Namespace(path) => path,
            Self::FixedRelative { request_path, .. } => request_path,
        }
    }
}
