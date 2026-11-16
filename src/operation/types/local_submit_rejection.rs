// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Why an owned local request was rejected before it started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalSubmitRejection {
    /// The manager or local executor is closed.
    Closed,
    /// A configured local capacity is exhausted.
    CapacityExceeded,
    /// The supplied operation manager does not own this executor.
    WrongManager,
    /// The operation scope ended before admission completed.
    ScopeEnded,
    /// The operation was stopped before local execution began.
    Stopped,
    /// The prepared local job was already claimed.
    AlreadyBound,
    /// A relative Host path requires the explicit asynchronous Host
    /// constructor.
    AuthorityNotCaptured,
}
