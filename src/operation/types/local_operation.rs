// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "async")]
use super::AsyncLocalOperation;
use crate::LocalFileError;
/// Handle for a typed local operation managed by qubit-operation.
///
/// Owned memory permits stay attached to this handle until its outcome is
/// delivered or the handle is dropped and the upstream result slot discarded.
///
/// # Type Parameters
/// - `T` — type carried through the operation contract.
/// - `E` — type carried through the operation contract.
pub struct LocalOperation<T: Send + 'static, E: Send + 'static = LocalFileError> {
    /// Stores the inner associated with this value.
    pub(in crate::operation::types) inner: Option<qubit_operation::OperationHandle<T, E>>,
    /// Stores the budget associated with this value.
    pub(in crate::operation::types) budget: Option<Arc<dyn Send + Sync>>,
}

impl<T: Send + 'static, E: Send + 'static> LocalOperation<T, E> {
    /// Runs the new behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `budget` — supplies data or policy for this operation.
    #[cfg(feature = "async")]
    pub(crate) fn new(inner: qubit_operation::OperationHandle<T, E>, budget: Arc<dyn Send + Sync>) -> Self {
        Self {
            inner: Some(inner),
            budget: Some(budget),
        }
    }
    /// Runs the id behavior while preserving the operation lifecycle
    /// invariants.
    pub fn id(&self) -> qubit_operation::OperationId {
        self.inner().id()
    }
    /// Runs the request stop behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `reason` — supplies data or policy for this operation.
    pub fn request_stop(&self, reason: qubit_operation::StopReason) -> qubit_operation::StopRequestStatus {
        self.inner().request_stop(reason)
    }
    /// Runs the snapshot behavior while preserving the operation lifecycle
    /// invariants.
    pub fn snapshot(&self) -> qubit_operation::OperationSnapshot {
        self.inner().snapshot()
    }
    /// Runs the wait outcome behavior while preserving the operation lifecycle
    /// invariants.
    pub fn wait_outcome(&mut self) -> Result<qubit_operation::OperationOutcome<T, E>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().wait_outcome();
        if result.is_ok() {
            self.budget.take();
        }
        result
    }
    /// Runs the try take outcome behavior while preserving the operation
    /// lifecycle invariants.
    pub fn try_take_outcome(
        &mut self,
    ) -> Result<Option<qubit_operation::OperationOutcome<T, E>>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().try_take_outcome();
        if matches!(result, Ok(Some(_))) {
            self.budget.take();
        }
        result
    }
    /// Runs the wait settlement behavior while preserving the operation
    /// lifecycle invariants.
    pub fn wait_settlement(&self) -> Result<Arc<qubit_operation::SettlementReport>, qubit_operation::ObservationError> {
        self.inner().wait_settlement()
    }
    /// Runs the wait settlement for behavior while preserving the operation
    /// lifecycle invariants.
    /// # Parameters
    /// - `timeout` — supplies data or policy for this operation.
    pub fn wait_settlement_for(
        &self,
        timeout: Duration,
    ) -> Result<qubit_operation::SettlementWait, qubit_operation::ObservationError> {
        self.inner().wait_settlement_for(timeout)
    }
    /// Runs the into observer behavior while preserving the operation lifecycle
    /// invariants.
    pub fn into_observer(mut self) -> qubit_operation::OperationObserver {
        let observer = self.inner.take().expect("local handle is present").into_observer();
        self.budget.take();
        observer
    }
    /// Runs the into async behavior while preserving the operation lifecycle
    /// invariants.
    #[cfg(feature = "async")]
    pub fn into_async(mut self) -> AsyncLocalOperation<T, E> {
        let inner = self.inner.take().expect("local handle is present").into_async();
        AsyncLocalOperation {
            inner: Some(inner),
            budget: self.budget.take(),
        }
    }
    /// Runs the inner behavior while preserving the operation lifecycle
    /// invariants.
    fn inner(&self) -> &qubit_operation::OperationHandle<T, E> {
        self.inner.as_ref().expect("local handle is present")
    }
    /// Runs the inner mut behavior while preserving the operation lifecycle
    /// invariants.
    fn inner_mut(&mut self) -> &mut qubit_operation::OperationHandle<T, E> {
        self.inner.as_mut().expect("local handle is present")
    }
}

impl<T: Send + 'static, E: Send + 'static> Drop for LocalOperation<T, E> {
    /// Runs the drop behavior while preserving the operation lifecycle
    /// invariants.
    fn drop(&mut self) {
        drop(self.inner.take());
        self.budget.take();
    }
}
