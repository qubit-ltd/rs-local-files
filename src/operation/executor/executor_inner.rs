// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_operation::OperationGroup;

use super::super::LocalExecutionConfig;
use super::super::internal::permits::DiskPermitPool;
use super::super::internal::permits::PermitPool;
use super::super::internal::worker::WorkerQueue;

/// Represents executor inner within the local operation subsystem.
pub(crate) struct ExecutorInner {
    /// Stores the group associated with this value.
    pub(crate) group: OperationGroup,
    /// Stores the config associated with this value.
    pub(crate) config: LocalExecutionConfig,
    /// Stores the queue associated with this value.
    pub(crate) queue: Arc<WorkerQueue>,
    /// Stores the queue slots associated with this value.
    pub(crate) queue_slots: PermitPool,
    /// Stores the waiter slots associated with this value.
    pub(crate) waiter_slots: PermitPool,
    /// Stores the byte slots associated with this value.
    pub(crate) byte_slots: PermitPool,
    /// Stores the handle slots associated with this value.
    pub(crate) handle_slots: PermitPool,
    /// Stores the disk slots associated with this value.
    #[allow(dead_code)]
    pub(crate) disk_slots: DiskPermitPool,
    /// Stores the fact slots associated with this value.
    pub(crate) fact_slots: PermitPool,
    /// Stores the cleanup slots associated with this value.
    #[allow(dead_code)]
    pub(crate) cleanup_slots: PermitPool,
}
