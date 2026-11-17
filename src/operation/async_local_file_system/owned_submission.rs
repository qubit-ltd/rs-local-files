// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::operation::LocalOperationOptions;

/// Admission requirements charged before an owned operation is submitted.
pub(super) struct OwnedSubmission {
    /// Whether the operation path is relative to the captured authority.
    pub(super) path_is_relative: bool,
    /// Native handles reserved for the operation.
    pub(super) required_handles: usize,
    /// Per-operation limits checked against executor capacity.
    pub(super) limits: LocalOperationOptions,
    /// Input bytes already owned before admission.
    pub(super) initial_owned_bytes: usize,
}
