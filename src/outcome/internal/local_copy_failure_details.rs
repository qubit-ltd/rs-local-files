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

use crate::LocalCopyFailureState;
use crate::LocalCopyStats;
use crate::LocalFileError;

/// Heap-owned details retained off [`crate::LocalCopyFailure`]'s hot path.
#[derive(Debug)]
pub(crate) struct LocalCopyFailureDetails {
    /// Source path supplied for the copy request.
    pub(crate) request_source_path: Option<PathBuf>,
    /// Destination path supplied for the copy request.
    pub(crate) request_target_path: Option<PathBuf>,
    /// Source entry being processed when the copy failed.
    pub(crate) failed_source_path: Option<PathBuf>,
    /// Destination entry being processed when the copy failed.
    pub(crate) failed_target_path: Option<PathBuf>,
    /// Primary typed filesystem error.
    pub(crate) error: LocalFileError,
    /// Most precise destination state proven by native operations.
    pub(crate) state: LocalCopyFailureState,
    /// Statistics accumulated before the failure.
    pub(crate) partial_stats: LocalCopyStats,
    /// Retained staging path only when its cleanup failed.
    pub(crate) staging_path: Option<PathBuf>,
    /// Secondary error that prevented staging removal or publication rollback.
    pub(crate) cleanup_error: Option<LocalFileError>,
}
