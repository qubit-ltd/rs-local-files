// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by temporary resource integration tests.
// qubit-style: allow coverage-cfg

use std::path::{
    Path,
    PathBuf,
};

/// Options for creating a cleanup-owned temporary file.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "temporary file options have no effect unless they are used"]
pub struct LocalTempFileOptions {
    /// Optional parent directory; the process temporary directory is the
    /// default.
    parent: Option<PathBuf>,
    /// Optional filename prefix.
    prefix: Option<String>,
    /// Optional filename suffix.
    suffix: Option<String>,
    /// Maximum random-name creation attempts.
    max_attempts: usize,
    /// Whether a missing parent directory is created before allocation.
    create_parent: bool,
}

impl LocalTempFileOptions {
    /// Creates default temporary-file options.
    pub const fn new() -> Self {
        Self {
            parent: None,
            prefix: None,
            suffix: None,
            max_attempts: 256,
            create_parent: false,
        }
    }

    /// Returns the configured parent, or `None` for the process temporary
    /// directory.
    #[must_use]
    pub fn parent(&self) -> Option<&Path> {
        self.parent.as_deref()
    }

    /// Returns the optional filename prefix.
    #[must_use]
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// Returns the optional filename suffix.
    #[must_use]
    pub fn suffix(&self) -> Option<&str> {
        self.suffix.as_deref()
    }

    /// Returns the maximum random-name creation attempts.
    #[must_use]
    pub const fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    /// Returns whether missing parent directories are created.
    #[must_use]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Enables recursive creation of a missing parent directory.
    pub const fn with_create_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets the native parent directory.
    ///
    /// # Parameters
    ///
    /// - `parent`: Absolute or relative parent directory.
    pub fn with_parent(mut self, parent: &Path) -> Self {
        self.parent = Some(parent.to_path_buf());
        self
    }

    /// Sets the portable filename prefix.
    ///
    /// # Parameters
    ///
    /// - `prefix`: Prefix validated before entry creation.
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_owned());
        self
    }

    /// Sets the portable filename suffix.
    ///
    /// # Parameters
    ///
    /// - `suffix`: Suffix validated before entry creation.
    pub fn with_suffix(mut self, suffix: &str) -> Self {
        self.suffix = Some(suffix.to_owned());
        self
    }

    /// Sets the maximum number of random-name attempts.
    ///
    /// # Parameters
    ///
    /// - `max_attempts`: Positive attempt count.
    pub const fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = max_attempts;
        self
    }
}

impl Default for LocalTempFileOptions {
    /// Returns default temporary-file options.
    fn default() -> Self {
        Self::new()
    }
}
