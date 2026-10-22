// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by directory integration tests.

/// Result of creating a native directory.
///
/// The outcome distinguishes a newly created directory from one that already
/// existed and was accepted by the request's policy.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
///
/// let filesystem = LocalFileSystem::host()?;
/// let directory = std::env::temp_dir().join(format!(
///     "qubit-create-directory-example-{}",
///     std::process::id(),
/// ));
/// let outcome = filesystem.create_directory(&directory)?;
/// assert!(outcome.created());
/// std::fs::remove_dir(directory)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalCreateDirectoryOutcome {
    /// Whether this operation created the requested directory.
    created: bool,
}

impl LocalCreateDirectoryOutcome {
    /// Creates a directory-creation outcome.
    ///
    /// # Parameters
    ///
    /// - `created`: Whether the requested entry was newly created.
    ///
    /// # Returns
    ///
    /// An outcome containing whether the operation created the directory.
    pub(crate) const fn new(created: bool) -> Self {
        Self { created }
    }

    /// Reports whether the requested directory was newly created.
    ///
    /// Returns `false` when an existing directory satisfied the request.
    #[must_use]
    #[inline]
    pub const fn created(self) -> bool {
        self.created
    }
}
