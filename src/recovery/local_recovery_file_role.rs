// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Role of a file retained after a conditional write attempt.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalRecoveryFileRole {
    /// Complete, independently verified copy of the candidate content.
    CandidateCopy,
    /// Staging content that may be incomplete or not yet independently verified.
    StagedCandidate,
    /// A file displaced from the target during publication.
    Displaced,
    /// A file displaced while attempting rollback.
    RollbackDisplaced,
}
