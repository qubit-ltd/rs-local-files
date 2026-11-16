// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::task::Waker;

/// Represents completion within the local operation subsystem.
/// Holds a worker result until its waiting future is polled.
///
/// # Type Parameters
/// - `T` — value produced by the worker.
pub(super) struct Completion<T> {
    /// Stores the value associated with this value.
    pub(super) value: Option<T>,
    /// Stores the waker associated with this value.
    pub(super) waker: Option<Waker>,
}
