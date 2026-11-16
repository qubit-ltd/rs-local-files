// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::task::Waker;

use qubit_operation::ChildOutcomeSummary;
use qubit_operation::ExecutionPhase;
use qubit_operation::OperationId;
use qubit_operation::OperationOutcome;
use qubit_operation::SettlementReport;
use qubit_operation::StopReason;

/// Stores the synchronized external-operation state and polling wakers.
///
/// # Type Parameters
/// - `T` — operation result type.
/// - `E` — operation error type.
/// Represents port within the local operation subsystem.
pub(super) struct Port<T, E> {
    /// Stores the id associated with this value.
    pub(super) id: Option<OperationId>,
    /// Stores the phase associated with this value.
    pub(super) phase: ExecutionPhase,
    /// Stores the stop associated with this value.
    pub(super) stop: Option<StopReason>,
    /// Stores the outcome associated with this value.
    pub(super) outcome: Option<OperationOutcome<T, E>>,
    /// Stores the summary associated with this value.
    pub(super) summary: Option<ChildOutcomeSummary>,
    /// Stores the released associated with this value.
    pub(super) released: bool,
    /// Stores the settlement associated with this value.
    pub(super) settlement: Option<Arc<SettlementReport>>,
    /// Stores the wakers associated with this value.
    pub(super) wakers: [Option<Waker>; 4],
}
