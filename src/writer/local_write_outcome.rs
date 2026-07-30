// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use super::LocalWriterState;

/// Structured result of committing or aborting a local writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalWriteOutcome {
    /// Terminal writer state.
    state: LocalWriterState,
    /// Whether destination publication was atomic.
    atomic: bool,
    /// Whether requested durability synchronization completed.
    durable: bool,
    /// Bytes accepted by the writer stream.
    bytes_written: u64,
}

impl LocalWriteOutcome {
    /// Creates a verified writer outcome.
    ///
    /// # Parameters
    ///
    /// - `state`: Terminal writer state.
    /// - `atomic`: Whether publication was atomic.
    /// - `durable`: Whether durability synchronization completed.
    /// - `bytes_written`: Bytes accepted by the stream.
    #[inline]
    pub(crate) const fn new(
        state: LocalWriterState,
        atomic: bool,
        durable: bool,
        bytes_written: u64,
    ) -> Self {
        Self {
            state,
            atomic,
            durable,
            bytes_written,
        }
    }

    /// Returns the terminal writer state.
    #[must_use]
    #[inline(always)]
    pub const fn state(self) -> LocalWriterState {
        self.state
    }

    /// Reports whether destination publication was atomic.
    #[must_use]
    #[inline(always)]
    pub const fn atomic(self) -> bool {
        self.atomic
    }

    /// Reports whether durability synchronization completed.
    #[must_use]
    #[inline(always)]
    pub const fn durable(self) -> bool {
        self.durable
    }

    /// Returns the number of bytes accepted by the writer stream.
    #[must_use]
    #[inline(always)]
    pub const fn bytes_written(self) -> u64 {
        self.bytes_written
    }
}
