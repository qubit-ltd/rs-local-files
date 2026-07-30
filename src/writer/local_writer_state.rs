// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

/// Observable state of a native writer publication session.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalWriterState {
    /// The byte stream remains writable.
    Open,
    /// Commit completed successfully.
    Committed,
    /// Abort completed without publishing staged content.
    Aborted,
    /// A failed commit did not publish the destination.
    NotPublished,
    /// Destination publication completed before a later failure.
    Published,
    /// The final byte or namespace state cannot be determined safely.
    Indeterminate,
}
