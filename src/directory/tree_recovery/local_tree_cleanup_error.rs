// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::local_tree_retained_resource::LocalTreeRetainedResource;
use crate::LocalFileError;
use crate::outcome::LocalTreeMutationState;

/// Cleanup failure that retains the identity-bound resource for recovery or
/// retry.
///
/// The cause is borrowed by [`Self::cause`] and exposed through the standard
/// error source chain. The optional retained owner is cleaned best-effort if
/// this error is dropped; extract it to adjust limits and retry explicitly.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeCleanupError;
///
/// fn parts(error: LocalTreeCleanupError) -> (
///     qubit_local_files::LocalFileError,
///     Vec<qubit_local_files::directory::LocalTreeRetainedResource>,
/// ) {
///     error.into_parts()
/// }
/// ```
#[derive(Debug)]
pub struct LocalTreeCleanupError {
    cause: LocalFileError,
    retained: Option<LocalTreeRetainedResource>,
    state: LocalTreeMutationState,
}
impl LocalTreeCleanupError {
    /// Builds the cleanup result while keeping its retained owner attached.
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
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Returns the owner policy retained by the recovery stage, when known.
    pub fn owner_access_policy(&self) -> Option<&crate::policy::LocalOwnerAccessPolicy> {
        self.retained
            .as_ref()
            .and_then(LocalTreeRetainedResource::owner_access_policy)
    }
    /// Reports whether cleanup had started mutating the tree before failing.
    pub const fn mutation_state(&self) -> LocalTreeMutationState {
        self.state
    }
    /// Returns cleanup owners that remain available for retry.
    ///
    /// The slice borrows the owners; they remain governed by this error's Drop
    /// behavior until ownership is transferred.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        self.retained.as_slice()
    }
    /// Transfers all retained cleanup owners to the caller.
    ///
    /// The error's primary cause is discarded. Any transferred owner remains
    /// armed and should be explicitly cleaned or deliberately retained.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        self.retained.take().into_iter().collect()
    }
    /// Transfers the cause and all retained cleanup owners.
    ///
    /// This tuple contains every transferable part of this error.
    pub fn into_parts(mut self) -> (LocalFileError, Vec<LocalTreeRetainedResource>) {
        let cause = std::mem::replace(
            &mut self.cause,
            LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Cleanup,
            ),
        );
        (cause, self.retained.take().into_iter().collect())
    }
}
impl std::fmt::Display for LocalTreeCleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tree cleanup failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreeCleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
