// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

use super::ConditionalEntry;
use crate::outcome::LocalConditionalPublicationState;
use crate::outcome::LocalWritePublicationMethod;

/// Outcome of a native operation, including observations made after failure.
#[must_use]
#[derive(Debug)]
pub(crate) struct NativeAttempt {
    /// Native publication protocol used by this attempt, independent of
    /// outcome.
    pub(crate) publication_method: LocalWritePublicationMethod,
    /// Native error, retained alongside any observed partial namespace change.
    pub(crate) error: Option<io::Error>,
    /// Observed publication state after the native attempt.
    pub(crate) publication: LocalConditionalPublicationState,
    /// Entry reopened from the final destination name, when available.
    pub(crate) installed: Option<ConditionalEntry>,
    /// Entries actually displaced by the native operation.
    pub(crate) displaced: Vec<ConditionalEntry>,
}
