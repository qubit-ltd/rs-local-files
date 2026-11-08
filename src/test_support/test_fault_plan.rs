// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Instance-local deterministic fault plans.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use super::TestFaultPoint;
use super::internal::TestFault;

/// FIFO fault plan scoped to one filesystem instance.
#[derive(Clone, Debug)]
pub struct TestFaultPlan {
    /// Shared FIFO retaining the remaining deterministic fault sequence.
    faults: Arc<Mutex<VecDeque<TestFault>>>,
}

impl TestFaultPlan {
    /// Creates a plan that fails once at `point` with `kind`.
    ///
    /// # Parameters
    ///
    /// - `point`: Operation boundary where the fault is injected.
    /// - `kind`: I/O error kind returned when the matching operation occurs.
    ///
    /// # Returns
    ///
    /// A plan containing one fault for the specified operation boundary.
    #[must_use]
    pub fn fail_once(point: TestFaultPoint, kind: io::ErrorKind) -> Self {
        let mut faults = VecDeque::new();
        faults.push_back(TestFault { point, kind });
        Self {
            faults: Arc::new(Mutex::new(faults)),
        }
    }

    /// Takes the next matching fault, if one remains.
    ///
    /// # Parameters
    ///
    /// - `point`: Operation boundary to match against the pending faults.
    ///
    /// # Returns
    ///
    /// `Some(error)` when a fault is pending for `point`; otherwise, `None`.
    pub(crate) fn take(&self, point: TestFaultPoint) -> Option<io::Error> {
        let mut faults = self.faults.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = faults.iter().position(|fault| fault.point == point)?;
        let fault = faults.remove(index)?;
        Some(io::Error::new(fault.kind, "injected local filesystem fault"))
    }
}
