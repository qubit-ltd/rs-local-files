// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt::Debug;

/// Selects which single stream operation a state machine coordinates.
/// Read and write states carry distinct queue invariants and reject operations
/// belonging to the other direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Direction {
    /// Polls the native reader and retains unread bytes for the next caller.
    Read,
    /// Queues native writes and preserves their ordering through flush/close.
    Write,
}
