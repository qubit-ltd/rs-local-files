// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use super::LocalWriteFailureState;
use super::LocalWriterState;
use crate::LocalWritePublicationMethod;

/// Structured result of committing or aborting a local writer.
///
/// [`Self::publication_method`] reports the backend method directly; callers
/// do not need to infer it from [`Self::atomic`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalWriteOutcome {
    /// Terminal writer state.
    state: LocalWriterState,
    /// Whether destination publication was atomic.
    atomic: bool,
    /// Native method used by the writer backend.
    publication_method: LocalWritePublicationMethod,
    /// Whether requested durability synchronization completed.
    durable: bool,
    /// Bytes accepted by the writer stream.
    bytes_written: usize,
    /// Failure state retained when a stream failure preceded terminal cleanup.
    failure_state: Option<LocalWriteFailureState>,
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
    pub(crate) const fn new(
        state: LocalWriterState,
        atomic: bool,
        publication_method: LocalWritePublicationMethod,
        durable: bool,
        bytes_written: usize,
        failure_state: Option<LocalWriteFailureState>,
    ) -> Self {
        Self {
            state,
            atomic,
            publication_method,
            durable,
            bytes_written,
            failure_state,
        }
    }

    /// Returns the terminal writer state.
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn state(self) -> LocalWriterState {
        self.state
    }

    /// Reports whether destination publication was atomic.
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn atomic(self) -> bool {
        self.atomic
    }

    /// Returns the native method used by this writer session.
    #[must_use = "the publication method must be inspected or stored"]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn publication_method(self) -> LocalWritePublicationMethod {
        self.publication_method
    }

    /// Reports whether durability synchronization completed.
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn durable(self) -> bool {
        self.durable
    }

    /// Returns the number of bytes accepted by the writer stream.
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn bytes_written(self) -> usize {
        self.bytes_written
    }

    /// Returns a failure state retained from an earlier stream or publication
    /// error, or `None` when the terminal outcome is fully successful.
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn failure_state(self) -> Option<LocalWriteFailureState> {
        self.failure_state
    }
}
