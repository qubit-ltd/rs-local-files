// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Operation stage at which a conditional commit stopped.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalConditionalWriteStage {
    /// Writer or staging setup.
    Prepare,
    /// Candidate content verification.
    VerifyCandidate,
    /// Independent candidate-copy creation.
    PrepareCandidateCopy,
    /// Candidate-copy verification.
    VerifyCandidateCopy,
    /// Target condition check.
    Precheck,
    /// Native publication operation.
    Publish,
    /// Displaced target verification.
    VerifyDisplaced,
    /// Installed target verification.
    VerifyInstalled,
    /// Rollback condition check.
    RollbackPrecheck,
    /// Native rollback operation.
    Rollback,
    /// Restored target verification.
    VerifyRollback,
    /// Required file or namespace synchronization.
    Sync,
}
