// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Deterministic boundaries in conditional publication, available to tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TestConditionalPoint {
    /// Before creating the independent candidate copy.
    BeforeCandidateCopy,
    /// Immediately before writing bytes to the unpublished candidate.
    BeforeWrite,
    /// After the independent candidate copy has been created.
    AfterCandidateCopy,
    /// Before checking the caller's precondition.
    BeforePrecheck,
    /// After the caller's precondition has been checked.
    AfterPrecheck,
    /// Immediately before the native publication operation.
    BeforePublish,
    /// Immediately after the native publication operation.
    AfterPublish,
    /// Before verifying the displaced destination entry.
    BeforeVerifyDisplaced,
    /// Before verifying the installed destination entry.
    BeforeVerifyInstalled,
    /// After the state machine validates a possible rollback.
    AfterRollbackPrecheck,
    /// After rollback's last backend validation and before its native exchange.
    BeforeRollbackExchange,
    /// After the rollback syscall attempt and before recovery receipts are
    /// collected.
    AfterRollback,
    /// Before the required file synchronization step.
    BeforeRequiredFileSync,
    /// Before the required parent-directory synchronization step.
    BeforeRequiredParentSync,
    /// After publication and required synchronization establish save
    /// completion.
    AfterSaveComplete,
    /// Before removing old displaced entries after successful publication.
    BeforeCleanupOld,
    /// Before removing the independent candidate copy after successful
    /// publication.
    BeforeCleanupCopy,
    /// Before synchronizing the namespace after cleanup.
    BeforeCleanupSync,
}
