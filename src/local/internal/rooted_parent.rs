// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Open rooted parent descriptors and their pending durability work.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::ffi::CString;
use std::fs::File;

/// An opened rooted destination parent and its pending durability work.
#[must_use = "rooted parent descriptors must be consumed by the requested operation"]
#[derive(Debug)]
pub(in crate::local) struct RootedParent {
    /// Final destination parent directory.
    directory: File,
    /// Final destination entry name.
    final_name: CString,
    /// Parents whose newly created child entries require synchronization.
    parent_dirs_to_sync: Vec<File>,
}

impl RootedParent {
    /// Creates a rooted parent result.
    ///
    /// # Parameters
    ///
    /// * `directory` - Final destination parent descriptor.
    /// * `final_name` - Final destination entry name.
    /// * `parent_dirs_to_sync` - Ancestor descriptors ordered shallowest to
    ///   deepest.
    ///
    /// # Returns
    ///
    /// A rooted parent result retaining all required descriptors.
    #[inline]
    pub(in crate::local) fn new(
        directory: File,
        final_name: CString,
        parent_dirs_to_sync: Vec<File>,
    ) -> Self {
        Self {
            directory,
            final_name,
            parent_dirs_to_sync,
        }
    }

    /// Decomposes this result into its owned descriptors and final name.
    ///
    /// # Returns
    ///
    /// The final parent, final entry name, and ancestor descriptors ordered
    /// shallowest to deepest.
    #[must_use = "the rooted parent descriptors, final name, and durability work must all be retained"]
    #[inline(always)]
    pub(in crate::local) fn into_parts(self) -> (File, CString, Vec<File>) {
        (self.directory, self.final_name, self.parent_dirs_to_sync)
    }
}
