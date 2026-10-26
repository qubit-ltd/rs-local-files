// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

use super::ConditionalEntry;
#[cfg(any(test, all(feature = "digest", feature = "test-support")))]
use super::ConditionalHooks;
use crate::outcome::LocalConditionalPublicationState;
use crate::outcome::LocalWritePublicationMethod;

/// Outcome of a native operation, including observations made after failure.
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[allow(dead_code)] // Includes checkpoints exercised only by the fake backend tests.
/// Native-operation boundaries where the commit protocol or deterministic
/// tests may observe or inject an outcome.
pub(crate) enum ConditionalPoint {
    Write,
    Flush,
    PrepareCandidateCopy,
    CandidateCopyCreated,
    VerifyCandidateCopy,
    SyncCandidate,
    SyncEntry,
    SyncNamespace,
    SyncInstalled,
    Precheck,
    AfterPrecheck,
    Publish,
    AfterPublish,
    VerifyDisplaced,
    VerifyInstalled,
    RollbackPrecheck,
    RollbackValidated,
    Rollback,
    RollbackCompleted,
    RollbackExchange,
    AfterRollbackSyscall,
    VerifyRollback,
    SyncRollback,
    SaveComplete,
    CleanupOld,
    CleanupCopy,
    CleanupSync,
    BeforeRequiredFileSync,
    BeforeRequiredParentSync,
}

/// One deterministic action consumed by a backend checkpoint.
///
/// Checkpoints let the shared commit protocol validate state transitions at
/// native-operation boundaries. Test hooks may inject a failure or competing
/// namespace change at those boundaries; production backends otherwise treat
/// them as no-ops.
#[derive(Clone, Debug)]
#[allow(dead_code)] // The fake backend constructs these actions in deterministic tests.
pub(crate) enum ConditionalHookAction {
    Noop,
    Fail(io::ErrorKind),
    ReplaceTarget(Vec<u8>),
    OverwriteTargetInPlace(Vec<u8>),
    CorruptCandidateCopy(Vec<u8>),
    PartialCandidateCopy(Vec<u8>),
    PartialPublish,
}

/// Native operations required by the generic conditional commit state machine.
pub(crate) trait ConditionalBackend: qubit_io::Output<Item = u8> {
    /// Returns the candidate staging entry.
    fn candidate(&self) -> &ConditionalEntry;
    /// Creates an independent copy using a fixed-size buffer in the controlled
    /// namespace.
    ///
    /// If copying fails after creating an entry, the backend registers that
    /// entry so `take_recovery_entries` can transfer it without resolving a
    /// path again.
    fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry>;
    /// Opens and identity-checks the current destination.
    fn open_target(&self) -> io::Result<ConditionalEntry>;
    /// Applies requested metadata from the observed destination to the
    /// candidate.
    fn preserve_metadata(&mut self, observed: &ConditionalEntry) -> io::Result<()>;
    /// Synchronizes candidate file content and metadata.
    fn sync_candidate(&self) -> io::Result<()>;
    /// Synchronizes a retained entry.
    fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()>;
    /// Synchronizes the relevant namespace and newly created parent chain.
    fn sync_namespace(&self) -> io::Result<()>;
    /// Checks whether an entry is the current candidate by native identity.
    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool>;
    /// Verifies that an entry still has its observed native identity and type.
    fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()>;
    /// Transfers guards to the publication-may-have-occurred state before a
    /// syscall.
    fn begin_publication(&mut self) -> io::Result<()>;
    /// Installs candidate only if the destination is absent.
    fn publish_absent(&mut self) -> NativeAttempt;
    /// Publishes candidate while retaining the displaced destination.
    fn publish_content(&mut self) -> NativeAttempt;
    /// Attempts one reverse publication using the observed displaced entry.
    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt;
    /// Removes only an entry whose identity was verified as safe to discard.
    fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()>;
    /// Cleans this backend's unpublished staging entries.
    fn abort(&mut self) -> io::Result<()>;
    /// Transfers retained recovery entries without resolving them by path
    /// again.
    fn take_recovery_entries(&mut self) -> Vec<ConditionalEntry>;
    /// Installs deterministic test hooks for backend checkpoints.
    #[cfg(any(test, all(feature = "digest", feature = "test-support")))]
    fn set_hooks(&mut self, hooks: ConditionalHooks);
    /// Runs a deterministic test checkpoint, or succeeds as a no-op in
    /// production.
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()>;
}
