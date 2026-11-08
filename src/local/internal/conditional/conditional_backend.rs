// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

use qubit_io::Output;

use super::ConditionalEntry;
#[cfg(any(test, all(feature = "digest", feature = "test-support")))]
use super::ConditionalHooks;
use super::ConditionalPoint;
use super::NativeAttempt;
use crate::LocalFileError;

/// Native operations required by the generic conditional commit state machine.
pub(crate) trait ConditionalBackend: Output<Item = u8> {
    /// Returns the candidate staging entry.
    ///
    /// # Returns
    ///
    /// The entry currently used to stage candidate content.
    #[must_use]
    fn candidate(&self) -> &ConditionalEntry;
    /// Creates an independent copy using a fixed-size buffer in the controlled
    /// namespace.
    ///
    /// If copying fails after creating an entry, the backend registers that
    /// entry so `take_recovery_entries` can transfer it without resolving a
    /// path again.
    ///
    /// # Returns
    ///
    /// The independent candidate copy created in the controlled namespace.
    ///
    /// # Errors
    ///
    /// Returns the native error when copying or registering the new entry
    /// fails.
    fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry>;
    /// Opens and identity-checks the current destination.
    ///
    /// # Returns
    ///
    /// The verified destination entry.
    ///
    /// # Errors
    ///
    /// Returns the native error when the destination cannot be opened or
    /// verified.
    fn open_target(&self) -> io::Result<ConditionalEntry>;
    /// Applies requested metadata from the observed destination to the
    /// candidate.
    ///
    /// # Parameters
    ///
    /// * `observed` is the destination entry whose requested metadata is
    ///   copied.
    ///
    /// # Errors
    ///
    /// Returns the native error when the requested metadata cannot be
    /// preserved.
    fn preserve_metadata(&mut self, observed: &ConditionalEntry) -> io::Result<()>;
    /// Synchronizes candidate file content and metadata.
    ///
    /// # Errors
    ///
    /// Returns the native error when candidate data or metadata cannot be
    /// synchronized.
    fn sync_candidate(&self) -> io::Result<()>;
    /// Synchronizes a retained entry.
    ///
    /// # Parameters
    ///
    /// * `entry` is the retained entry to synchronize.
    ///
    /// # Errors
    ///
    /// Returns the native error when the retained entry cannot be synchronized.
    fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()>;
    /// Synchronizes the relevant namespace and newly created parent chain.
    ///
    /// # Errors
    ///
    /// Returns the native error when the relevant namespace cannot be
    /// synchronized.
    fn sync_namespace(&self) -> io::Result<()>;
    /// Checks whether an entry is the current candidate by native identity.
    ///
    /// # Parameters
    ///
    /// * `entry` is the observed entry to compare with the candidate.
    ///
    /// # Returns
    ///
    /// Whether the entry has the candidate's native identity.
    ///
    /// # Errors
    ///
    /// Returns the native error when identity comparison fails.
    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool>;
    /// Verifies that an entry still has its observed native identity and type.
    ///
    /// # Parameters
    ///
    /// * `entry` is the entry whose identity and type must remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns the native error when the entry no longer matches its
    /// observation.
    fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()>;
    /// Transfers guards to the publication-may-have-occurred state before a
    /// syscall.
    ///
    /// # Errors
    ///
    /// Returns the native error when guards cannot be transferred safely.
    fn begin_publication(&mut self) -> io::Result<()>;
    /// Installs candidate only if the destination is absent.
    ///
    /// # Returns
    ///
    /// The native publication outcome and any observed namespace changes.
    fn publish_absent(&mut self) -> NativeAttempt;
    /// Publishes candidate while retaining the displaced destination.
    ///
    /// # Returns
    ///
    /// The native publication outcome and any observed namespace changes.
    fn publish_content(&mut self) -> NativeAttempt;
    /// Attempts one reverse publication using the observed displaced entry.
    ///
    /// # Parameters
    ///
    /// * `displaced` is the verified entry to restore as the destination.
    ///
    /// # Returns
    ///
    /// The outcome of the single reverse-publication attempt.
    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt;
    /// Removes only an entry whose identity was verified as safe to discard.
    ///
    /// # Parameters
    ///
    /// * `entry` is the verified entry to remove.
    ///
    /// # Errors
    ///
    /// Returns the native error when the verified entry cannot be removed.
    fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()>;
    /// Cleans this backend's unpublished staging entries.
    ///
    /// # Errors
    ///
    /// Returns the native error when an unpublished staging entry cannot be
    /// cleaned.
    fn abort(&mut self) -> io::Result<()>;
    /// Transfers retained recovery entries without resolving them by path
    /// again.
    ///
    /// # Returns
    ///
    /// The retained entries transferred to the caller.
    #[must_use]
    fn take_recovery_entries(&mut self) -> Vec<ConditionalEntry>;
    /// Transfers cleanup errors that do not correspond to retained files.
    ///
    /// # Returns
    ///
    /// The cleanup errors transferred from this backend.
    #[must_use]
    fn take_cleanup_errors(&mut self) -> Vec<LocalFileError> {
        Vec::new()
    }
    /// Installs deterministic test hooks for backend checkpoints.
    ///
    /// # Parameters
    ///
    /// * `hooks` supplies the checkpoint actions for this backend.
    #[cfg(any(test, all(feature = "digest", feature = "test-support")))]
    fn set_hooks(&mut self, hooks: ConditionalHooks);
    /// Runs a deterministic test checkpoint, or succeeds as a no-op in
    /// production.
    ///
    /// # Parameters
    ///
    /// * `point` identifies the checkpoint to execute.
    ///
    /// # Errors
    ///
    /// Returns the injected or native error when the checkpoint fails.
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()>;
}
