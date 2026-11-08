// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::local_tree_retained_resource::LocalTreeRetainedResource;
use crate::LocalFileError;

/// Failure while applying a staged tree access policy.
///
/// The cause is borrowed by [`Self::cause`] and exposed through the standard
/// source chain. Any retained cleanup owners remain owned by this error until
/// extracted; dropping an armed owner can trigger best-effort cleanup. Methods
/// that transfer only owners also discard the cause and access state; use
/// [`Self::into_parts`] when those values are needed.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalTreeAccessError;
///
/// fn parts(error: LocalTreeAccessError) -> (
///     qubit_local_files::LocalFileError,
///     qubit_local_files::outcome::LocalTreeAccessState,
///     Vec<qubit_local_files::directory::LocalTreeRetainedResource>,
/// ) {
///     error.into_parts()
/// }
/// ```
#[derive(Debug)]
pub struct LocalTreeAccessError {
    /// Original filesystem failure that caused the access-policy operation
    /// to fail.
    cause: LocalFileError,
    /// Access-policy state observed when the operation failed.
    access_state: crate::outcome::LocalTreeAccessState,
    /// Cleanup owners retained so callers can inspect or recover resources.
    retained: Vec<LocalTreeRetainedResource>,
}
impl LocalTreeAccessError {
    /// Creates an error with no retained cleanup owners.
    ///
    /// The caller supplies the underlying filesystem failure and the policy
    /// state observed at failure.
    ///
    /// # Parameters
    ///
    /// * `cause` is the filesystem failure being wrapped.
    /// * `access_state` is the policy state captured at failure.
    pub(crate) fn new(cause: LocalFileError, access_state: crate::outcome::LocalTreeAccessState) -> Self {
        Self {
            cause,
            access_state,
            retained: Vec::new(),
        }
    }
    /// Returns the access-policy failure cause.
    ///
    /// # Returns
    ///
    /// A borrow of the underlying filesystem failure.
    #[inline]
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }
    /// Returns the access state observed when the operation failed.
    ///
    /// # Returns
    ///
    /// The policy state captured at failure.
    #[inline]
    pub const fn access_state(&self) -> crate::outcome::LocalTreeAccessState {
        self.access_state
    }
    /// Borrows cleanup owners that remain held by this error.
    ///
    /// # Returns
    ///
    /// A slice of owners retained for cleanup or recovery.
    #[inline]
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers retained cleanup owners and discards the cause and access
    /// state.
    ///
    /// # Returns
    ///
    /// The retained owners, leaving the consumed error's cause and state
    /// unavailable to the caller.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
    /// Transfers the cause, access state, and retained cleanup owners.
    ///
    /// # Returns
    ///
    /// The original cause, captured state, and retained owners in that order.
    pub fn into_parts(
        mut self,
    ) -> (
        LocalFileError,
        crate::outcome::LocalTreeAccessState,
        Vec<LocalTreeRetainedResource>,
    ) {
        (self.cause, self.access_state, std::mem::take(&mut self.retained))
    }
}
impl std::fmt::Display for LocalTreeAccessError {
    /// Formats the access-policy failure together with its underlying cause.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter that receives the error message.
    ///
    /// # Errors
    ///
    /// Returns the formatter's error if writing the message fails.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tree access policy failed: {}", self.cause)
    }
}
impl std::error::Error for LocalTreeAccessError {
    /// Exposes the underlying filesystem failure to standard error-chain
    /// consumers.
    ///
    /// # Returns
    ///
    /// Always `Some`, containing the wrapped filesystem failure.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
