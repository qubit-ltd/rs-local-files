// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared state for one installed test gate.

use std::sync::Condvar;
use std::sync::Mutex;

/// Coordinates entry and release between a worker and its test thread.
#[derive(Default)]
pub(super) struct GateState {
    /// Whether the worker entered and whether the test released the gate.
    pub(super) phase: Mutex<(bool, bool)>,
    /// Notifies the other side when either phase changes.
    pub(super) changed: Condvar,
}
