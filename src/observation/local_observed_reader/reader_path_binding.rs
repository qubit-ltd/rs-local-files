// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::LocalResult;
use crate::error::LocalFileOperation;
use crate::observation::LocalSourceVerification;
use crate::resolution::internal::checked_path_binding_deadline;

/// Internal contract for checking whether an opened path still names its
/// source.
///
/// Implementations share one absolute deadline when multiple bindings are
/// combined, and report namespace changes as successful verification states.
pub(crate) trait ReaderPathBinding: std::fmt::Debug + Send + Sync {
    /// Returns the maximum duration allowed for this binding's verification.
    ///
    /// # Returns
    /// The duration limit, or `None` when verification has no time limit.
    fn verification_deadline(&self) -> Option<std::time::Duration>;

    /// Verifies the binding using a deadline shared with any combined bindings.
    ///
    /// # Parameters
    /// - `deadline`: Absolute monotonic deadline shared by combined bindings.
    ///
    /// # Returns
    /// The observed source verification state.
    ///
    /// # Errors
    /// Returns structured errors when verification cannot be completed.
    fn verify_until(&self, deadline: Option<std::time::Instant>) -> LocalResult<LocalSourceVerification>;

    /// Computes the deadline from the current instant, then verifies the
    /// binding.
    ///
    /// # Returns
    /// The observed source verification state.
    ///
    /// # Errors
    /// Returns a deadline overflow or a structured verification error.
    fn verify(&self) -> LocalResult<LocalSourceVerification> {
        let deadline = checked_path_binding_deadline(
            std::time::Instant::now(),
            self.verification_deadline(),
            LocalFileOperation::Metadata,
            None,
        )?;
        self.verify_until(deadline)
    }
}
