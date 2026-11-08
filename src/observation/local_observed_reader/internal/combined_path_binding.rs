// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;
use std::time::Instant;

use super::ReaderPathBinding;
use crate::LocalResult;
use crate::observation::LocalSourceVerification;

/// Verifies multiple path bindings under the strictest shared deadline.
#[derive(Debug)]
pub(super) struct CombinedPathBinding {
    /// Bindings checked in insertion order using the same deadline.
    pub(super) bindings: Vec<Box<dyn ReaderPathBinding>>,
}

impl ReaderPathBinding for CombinedPathBinding {
    /// Uses the shortest configured duration so every child shares the limit.
    ///
    /// # Returns
    /// The shortest child duration, or `None` when no child sets a limit.
    fn verification_deadline(&self) -> Option<Duration> {
        self.bindings
            .iter()
            .filter_map(|binding| binding.verification_deadline())
            .min()
    }

    /// Verifies children in order, returning changed immediately and otherwise
    /// preserving a missing result if any child is missing.
    ///
    /// # Parameters
    /// - `deadline`: Monotonic deadline shared by every child binding.
    ///
    /// # Returns
    /// `Changed` immediately on a changed child, otherwise `Missing` if any
    /// child is missing, and `Unchanged` if all children are unchanged.
    ///
    /// # Errors
    /// Propagates the first child verification error.
    fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
        let mut missing = false;
        for binding in &self.bindings {
            match binding.verify_until(deadline)? {
                LocalSourceVerification::Changed => return Ok(LocalSourceVerification::Changed),
                LocalSourceVerification::Missing => missing = true,
                LocalSourceVerification::Unchanged => {}
            }
        }
        Ok(if missing {
            LocalSourceVerification::Missing
        } else {
            LocalSourceVerification::Unchanged
        })
    }
}
