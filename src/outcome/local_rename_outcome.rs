// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by rename integration tests.

/// Guarantees actually achieved by a native rename.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// let filesystem = LocalFileSystem::host()?;
/// let directory = std::env::temp_dir();
/// let source = directory.join(format!("qubit-rename-source-{}", std::process::id()));
/// let target = directory.join(format!("qubit-rename-target-{}", std::process::id()));
/// std::fs::write(&source, b"rename example")?;
/// let outcome = filesystem.rename(Path::new(&source), Path::new(&target))?;
/// assert!(outcome.atomic());
/// std::fs::remove_file(target)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalRenameOutcome {
    /// Whether namespace replacement was atomic.
    atomic: bool,
    /// Whether the destination parent was synchronized.
    durable: bool,
}

impl LocalRenameOutcome {
    /// Creates a rename outcome.
    ///
    /// # Parameters
    ///
    /// - `atomic`: Whether the namespace change was atomic.
    /// - `durable`: Whether parent-directory durability was synchronized.
    ///
    /// # Returns
    ///
    /// An outcome recording the atomicity and durability guarantees achieved
    /// by the rename operation.
    pub(crate) const fn new(atomic: bool, durable: bool) -> Self {
        Self { atomic, durable }
    }

    /// Reports whether the namespace change was atomic.
    #[must_use]
    #[inline]
    pub const fn atomic(self) -> bool {
        self.atomic
    }

    /// Reports whether parent-directory durability was synchronized.
    #[must_use]
    #[inline]
    pub const fn durable(self) -> bool {
        self.durable
    }
}
