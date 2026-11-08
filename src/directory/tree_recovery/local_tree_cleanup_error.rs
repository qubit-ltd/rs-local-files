// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::mem;

use super::local_tree_retained_resource::LocalTreeRetainedResource;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::outcome::LocalTreeMutationState;
use crate::policy::LocalOwnerAccessPolicy;

/// A cleanup failure together with the identity-bound resource needed for
/// recovery or retry.
///
/// The cause is borrowed by [`Self::cause`] and exposed through the standard
/// error source chain. The retained owner remains attached until extracted;
/// dropping this error allows its owner to make a final best-effort cleanup
/// attempt.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeCleanupError;
/// use qubit_local_files::directory::LocalTreeRetainedResource;
/// use qubit_local_files::LocalFileError;
///
/// // Pass the result of `LocalStagedDirectory::cleanup` to this handler.
/// fn recover(result: Result<(), LocalTreeCleanupError>) {
///     if let Err(error) = result {
///         let (cause, retained): (LocalFileError, Vec<LocalTreeRetainedResource>) =
///             error.into_parts();
///         eprintln!("cleanup failed: {cause}");
///         drop(retained); // Dropping owners makes their best-effort cleanup attempt.
///     }
/// }
/// ```
#[must_use = "cleanup failures should be handled or deliberately discarded"]
#[derive(Debug)]
pub struct LocalTreeCleanupError {
    /// The primary error that caused cleanup to stop.
    cause: LocalFileError,
    /// The owner retained for explicit retry or best-effort cleanup on drop.
    retained: Option<LocalTreeRetainedResource>,
    /// The tree mutation progress recorded when cleanup failed.
    state: LocalTreeMutationState,
}
impl LocalTreeCleanupError {
    /// Creates a cleanup error while preserving its owner for recovery.
    ///
    /// # Parameters
    ///
    /// * `cause` - The primary failure that stopped cleanup.
    /// * `state` - The tree mutation progress observed before failure.
    /// * `retained` - The identity-bound owner available for retry or cleanup.
    ///
    /// # Returns
    ///
    /// An error that retains the supplied owner until it is extracted or
    /// dropped.
    pub(super) fn new(
        cause: LocalFileError,
        state: LocalTreeMutationState,
        retained: LocalTreeRetainedResource,
    ) -> Self {
        Self {
            cause,
            retained: Some(retained),
            state,
        }
    }

    /// Returns the primary cleanup failure without consuming retained owners.
    ///
    /// # Returns
    ///
    /// A borrow of the primary cleanup failure.
    #[must_use = "the primary cleanup failure should be inspected"]
    #[inline]
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Returns the owner policy retained by the recovery stage, when known.
    ///
    /// # Returns
    ///
    /// The retained policy, or `None` if no policy was attached to the owner.
    #[must_use = "the retained owner policy should be inspected"]
    #[inline]
    pub fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.retained
            .as_ref()
            .and_then(LocalTreeRetainedResource::owner_access_policy)
    }
    /// Reports whether cleanup had started mutating the tree before failing.
    ///
    /// # Returns
    ///
    /// The mutation progress recorded when cleanup failed.
    #[must_use = "the cleanup mutation progress should be inspected"]
    #[inline]
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.state
    }
    /// Returns cleanup owners that remain available for retry.
    ///
    /// The slice borrows the owners; they remain governed by this error's Drop
    /// behavior until ownership is transferred.
    ///
    /// # Returns
    ///
    /// A borrowed slice of the retained cleanup owners, empty if none remain.
    #[must_use = "the retained cleanup owners should be inspected"]
    #[inline]
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        self.retained.as_slice()
    }
    /// Transfers all retained cleanup owners to the caller.
    ///
    /// The error's primary cause is discarded. Any transferred owner remains
    /// armed and should be explicitly cleaned or deliberately retained.
    ///
    /// # Returns
    ///
    /// All owners retained for cleanup, or an empty vector if none remain.
    #[must_use = "the retained cleanup owners should be handled"]
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        self.retained.take().into_iter().collect()
    }
    /// Transfers the cause and all retained cleanup owners.
    ///
    /// This tuple contains every transferable part of this error.
    ///
    /// # Returns
    ///
    /// The primary cause and all retained cleanup owners.
    #[must_use = "the cause and retained cleanup owners should be handled"]
    pub fn into_parts(mut self) -> (LocalFileError, Vec<LocalTreeRetainedResource>) {
        let cause = mem::replace(
            &mut self.cause,
            LocalFileError::new(LocalFileErrorKind::InvalidState, LocalFileOperation::Cleanup),
        );
        (cause, self.retained.take().into_iter().collect())
    }
}
impl Display for LocalTreeCleanupError {
    /// Formats the cleanup failure using its primary cause.
    ///
    /// # Parameters
    ///
    /// * `f` - The formatter that receives the cleanup failure description.
    ///
    /// # Returns
    ///
    /// `Ok(())` when formatting succeeds, or the formatter's error otherwise.
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "tree cleanup failed: {}", self.cause)
    }
}
impl Error for LocalTreeCleanupError {
    /// Returns the primary cleanup failure for standard error chaining.
    ///
    /// # Returns
    ///
    /// The primary cause wrapped in `Some`.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}
