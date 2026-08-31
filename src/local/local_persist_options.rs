// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Temporary-file persistence options.
// qubit-style: allow source-test-pair

/// Options controlling temporary file persistence behavior.
///
/// The default is conservative: existing destination paths are not overwritten.
/// Construct this non-exhaustive type through [`Self::new`] and its builder.
/// Builder results must be used:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::options::LocalPersistOptions;
///
/// LocalPersistOptions::new().with_overwrite();
/// ```
///
/// Configuration fields are private:
///
/// ```compile_fail
/// use qubit_local_files::options::LocalPersistOptions;
///
/// let mut options = LocalPersistOptions::default();
/// options.overwrite = true;
/// ```
#[must_use = "persistence options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalPersistOptions {
    /// Whether an existing target path may be overwritten.
    overwrite: bool,
    /// Whether a missing target parent may be created before publication.
    create_parent: bool,
}

impl LocalPersistOptions {
    /// Returns conservative persistence options.
    ///
    /// # Returns
    /// Options that reject existing destination paths.
    pub const fn new() -> Self {
        Self {
            overwrite: false,
            create_parent: false,
        }
    }

    /// Returns whether an existing target may be overwritten.
    ///
    /// # Returns
    /// `true` when target replacement is enabled.
    #[must_use]
    pub const fn overwrites(&self) -> bool {
        self.overwrite
    }

    /// Returns whether missing target parents may be created.
    #[must_use]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Enables recursive creation of a missing target parent.
    pub const fn with_create_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Enables replacement of an existing target path.
    ///
    /// # Returns
    /// Updated persistence options that permit overwriting.
    pub const fn with_overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }
}

impl Default for LocalPersistOptions {
    /// Returns conservative persistence options.
    ///
    /// # Returns
    /// Options that reject existing destination paths.
    fn default() -> Self {
        Self::new()
    }
}
