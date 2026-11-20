// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::super::permits::Permit;

/// Rejected cleanup task and the permit returned with it.
pub(crate) type CleanupEnqueueError = (Box<dyn FnOnce() + Send + 'static>, Permit);
/// Rejected cleanup job, permit, and its completion callback.
pub(crate) type CleanupCompletionEnqueueError = (
    Box<dyn FnOnce() + Send + 'static>,
    Permit,
    Box<dyn FnOnce() + Send + 'static>,
);
