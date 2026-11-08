// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::policy::LocalFileSyncRequirement;

/// Options controlling the synchronization requested when sealing a temp file.
///
/// Use this type to select whether sealing should request synchronization of
/// the temporary file before publishing it. The default requires
/// synchronization.
///
/// # Examples
///
/// ```
/// use qubit_local_files::policy::LocalFileSyncRequirement;
/// use qubit_local_files::LocalSealOptions;
///
/// let options = LocalSealOptions::default()
///     .with_file_sync(LocalFileSyncRequirement::Preferred);
/// assert_eq!(options.file_sync(), LocalFileSyncRequirement::Preferred);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "seal options have no effect unless they are used"]
pub struct LocalSealOptions {
    /// Synchronization level requested before the sealed file is published.
    file_sync: LocalFileSyncRequirement,
}

impl LocalSealOptions {
    /// Creates seal options that require file synchronization.
    #[inline]
    pub const fn new() -> Self {
        Self {
            file_sync: LocalFileSyncRequirement::Required,
        }
    }

    /// Returns the synchronization requirement used when sealing the file.
    ///
    /// The returned value is copied from the options and does not perform I/O.
    pub const fn file_sync(&self) -> LocalFileSyncRequirement {
        self.file_sync
    }

    /// Sets the synchronization requirement used when sealing the file.
    ///
    /// # Parameters
    ///
    /// * `requirement` — whether synchronization is required, preferred, or not
    ///   requested.
    ///
    /// # Returns
    ///
    /// The updated options value, allowing further configuration to be chained.
    #[inline]
    pub const fn with_file_sync(mut self, requirement: LocalFileSyncRequirement) -> Self {
        self.file_sync = requirement;
        self
    }
}

impl Default for LocalSealOptions {
    /// Returns options that require file synchronization when sealing.
    fn default() -> Self {
        Self::new()
    }
}
