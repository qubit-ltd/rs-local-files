// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Heap-owned storage for one failed copy operation.
// qubit-style: allow source-test-pair
// Covered through the public LocalCopyFailure integration tests.

use std::path::PathBuf;

use crate::{
    LocalCopyFailureState,
    LocalCopyStats,
    LocalFileError,
};

/// Heap-owned details retained off [`crate::LocalCopyFailure`]'s hot path.
#[derive(Debug)]
pub(crate) struct LocalCopyFailureDetails {
    /// Primary typed filesystem error.
    pub(crate) error: LocalFileError,
    /// Most precise destination state proven by native operations.
    pub(crate) state: LocalCopyFailureState,
    /// Statistics accumulated before the failure.
    pub(crate) partial_stats: LocalCopyStats,
    /// Retained staging path only when its cleanup failed.
    pub(crate) staging_path: Option<PathBuf>,
    /// Secondary cleanup error that prevented staging removal.
    pub(crate) cleanup_error: Option<LocalFileError>,
}
