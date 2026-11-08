// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt::Debug;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use super::TestConditionalPoint;

/// Callback owned by a test hook and consumed at most once.
type TestAction = Box<dyn FnOnce() -> io::Result<()> + Send>;

/// One-shot action attached to one conditional publication boundary.
#[must_use = "conditional hooks must be attached to a publication operation"]
pub struct TestConditionalHooks {
    /// Publication boundary where the callback is invoked.
    point: TestConditionalPoint,
    /// Shared storage that takes the callback when it is first invoked.
    action: Arc<Mutex<Option<TestAction>>>,
}

impl Debug for TestConditionalHooks {
    /// Formats the hook's publication point without exposing its callback.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Destination that receives the debug representation.
    ///
    /// # Errors
    ///
    /// Returns the formatter's error if writing the debug representation fails.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        formatter
            .debug_struct("TestConditionalHooks")
            .field("point", &self.point)
            .finish_non_exhaustive()
    }
}

impl TestConditionalHooks {
    /// Creates a hook that runs `action` once when `point` is reached.
    ///
    /// # Type Parameters
    ///
    /// * `F` - A sendable, one-shot callback that returns an I/O result.
    ///
    /// # Parameters
    ///
    /// * `point` - Publication boundary that triggers the callback.
    /// * `action` - Callback invoked at that boundary.
    ///
    /// # Returns
    ///
    /// A hook storing the callback in shared, one-shot state.
    pub fn once<F>(point: TestConditionalPoint, action: F) -> Self
    where
        F: FnOnce() -> io::Result<()> + Send + 'static,
    {
        Self {
            point,
            action: Arc::new(Mutex::new(Some(Box::new(action)))),
        }
    }

    /// Returns the checkpoint and shared callback state for backend
    /// installation.
    ///
    /// # Returns
    ///
    /// The publication point and the mutex-protected, one-shot callback slot.
    #[must_use]
    pub(crate) fn into_parts(self) -> (TestConditionalPoint, Arc<Mutex<Option<TestAction>>>) {
        (self.point, self.action)
    }
}
