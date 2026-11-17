// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "async")]
use std::sync::Arc;

#[cfg(feature = "async")]
use super::LocalOperation;
#[cfg(feature = "async")]
use crate::LocalFileError;
/// Owns an asynchronous local operation result and its memory reservation.
///
/// # Type Parameters
/// - `T` — result type delivered by the operation.
/// - `E` — error type delivered when the operation fails.
#[cfg(feature = "async")]
pub struct AsyncLocalOperation<T: Send + 'static, E: Send + 'static = LocalFileError> {
    /// Stores the inner associated with this value.
    pub(in crate::operation::types) inner: Option<qubit_operation::AsyncOperationHandle<T, E>>,
    /// Stores the budget associated with this value.
    pub(in crate::operation::types) budget: Option<Arc<dyn Send + Sync>>,
}

#[cfg(feature = "async")]
impl<T: Send + 'static, E: Send + 'static> AsyncLocalOperation<T, E> {
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
    pub async fn wait_outcome(
        &mut self,
    ) -> Result<qubit_operation::OperationOutcome<T, E>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().wait_outcome().await;
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
    pub async fn wait_settlement(&self) -> Arc<qubit_operation::SettlementReport> {
        self.inner().wait_settlement().await
    }
    /// Runs the into sync behavior while preserving the operation lifecycle
    /// invariants.
    pub fn into_sync(mut self) -> LocalOperation<T, E> {
        let inner = self.inner.take().expect("async local handle is present").into_sync();
        LocalOperation {
            inner: Some(inner),
            budget: self.budget.take(),
        }
    }
    /// Runs the into observer behavior while preserving the operation lifecycle
    /// invariants.
    pub fn into_observer(mut self) -> qubit_operation::AsyncOperationObserver {
        let observer = self
            .inner
            .take()
            .expect("async local handle is present")
            .into_observer();
        self.budget.take();
        observer
    }
    /// Runs the inner behavior while preserving the operation lifecycle
    /// invariants.
    fn inner(&self) -> &qubit_operation::AsyncOperationHandle<T, E> {
        self.inner.as_ref().expect("async local handle is present")
    }
    /// Runs the inner mut behavior while preserving the operation lifecycle
    /// invariants.
    fn inner_mut(&mut self) -> &mut qubit_operation::AsyncOperationHandle<T, E> {
        self.inner.as_mut().expect("async local handle is present")
    }
}

#[cfg(feature = "async")]
impl<T: Send + 'static, E: Send + 'static> Drop for AsyncLocalOperation<T, E> {
    /// Runs the drop behavior while preserving the operation lifecycle
    /// invariants.
    fn drop(&mut self) {
        drop(self.inner.take());
        self.budget.take();
    }
}
