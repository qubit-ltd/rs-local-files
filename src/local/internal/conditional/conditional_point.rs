// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Native-operation boundaries where the commit protocol or deterministic
/// tests may observe or inject an outcome.
///
/// A hook at a checkpoint observes or injects behavior immediately around the
/// corresponding native operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[allow(dead_code)] // Includes checkpoints exercised only by the fake backend tests.
pub(crate) enum ConditionalPoint {
    /// The output writer accepted bytes.
    Write,
    /// The output writer flushed buffered bytes.
    Flush,
    /// Candidate-copy creation is about to begin.
    PrepareCandidateCopy,
    /// The candidate copy was created.
    CandidateCopyCreated,
    /// The candidate copy is being identity-checked.
    VerifyCandidateCopy,
    /// Candidate content and metadata are being synchronized.
    SyncCandidate,
    /// A retained entry is being synchronized.
    SyncEntry,
    /// The controlled namespace is being synchronized.
    SyncNamespace,
    /// The installed entry is being synchronized.
    SyncInstalled,
    /// The destination precheck is beginning.
    Precheck,
    /// The destination precheck completed.
    AfterPrecheck,
    /// Candidate publication is about to begin.
    Publish,
    /// Candidate publication completed.
    AfterPublish,
    /// The displaced entry is being verified.
    VerifyDisplaced,
    /// The installed entry is being verified.
    VerifyInstalled,
    /// Rollback precheck is beginning.
    RollbackPrecheck,
    /// Rollback precheck completed.
    RollbackValidated,
    /// Rollback publication is about to begin.
    Rollback,
    /// Rollback completed.
    RollbackCompleted,
    /// Rollback exchange is about to begin.
    RollbackExchange,
    /// The native rollback syscall returned.
    AfterRollbackSyscall,
    /// The restored entry is being verified.
    VerifyRollback,
    /// The restored entry is being synchronized.
    SyncRollback,
    /// The transaction has reached its saved-complete state.
    SaveComplete,
    /// The displaced entry is being cleaned up.
    CleanupOld,
    /// The candidate copy is being cleaned up.
    CleanupCopy,
    /// Cleanup namespace changes are being synchronized.
    CleanupSync,
    /// Required file synchronization is about to occur.
    BeforeRequiredFileSync,
    /// Required parent synchronization is about to occur.
    BeforeRequiredParentSync,
}
