// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Exact native names rejected by fixed-directory operations.
#[derive(Clone, Debug, Default)]
pub struct LocalComponentPolicy {
    denied: Vec<OsString>,
}

impl LocalComponentPolicy {
    /// Allows every component name.
    pub const fn allow_all() -> Self {
        Self { denied: Vec::new() }
    }

    /// Rejects the exact native names supplied by the caller.
    pub fn deny_names(names: Vec<OsString>) -> LocalResult<Self> {
        for name in &names {
            let mut components = Path::new(name).components();
            let one_normal_component = matches!(components.next(), Some(Component::Normal(value)) if value == name)
                && components.next().is_none();
            if name.is_empty() || name == "." || name == ".." || !one_normal_component {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::BindPath)
                        .with_reason("denied component names must be non-empty single native names other than . or .."),
                );
            }
        }
        Ok(Self { denied: names })
    }

    pub(crate) fn denies(&self, component: &std::ffi::OsStr) -> bool {
        self.denied.iter().any(|name| name == component)
    }

    #[cfg(not(unix))]
    pub(crate) fn is_allow_all(&self) -> bool {
        self.denied.is_empty()
    }

    pub(crate) fn validate_component(&self, component: &std::ffi::OsStr) -> LocalResult<()> {
        if self.denies(component) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                    .with_reason("path component is rejected by the configured component policy"),
            );
        }
        Ok(())
    }
}
