// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::task::Context;
use std::task::Poll;

use super::LocalStreamCompletion;
use super::LocalStreamRequest;
/// Non-blocking driver implemented by a local serial worker or a deterministic
/// fake.
///
/// Implementations should arrange for command progress to wake the supplied
/// task.
///
/// For a given request id, a driver must schedule at most one command and keep
/// its completion observable until it returns `Ready` from this method. Calls
/// for one resource are serialized by the stream handle. A driver must not
/// retain caller buffers: write payloads are shared owned allocations and read
/// storage is allocated by the driver/worker.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalStreamDriver;
/// use qubit_local_files::operation::stream::LocalStreamResource;
///
/// fn make_port<D: LocalStreamDriver>(driver: D) -> LocalStreamResource {
///     LocalStreamResource::new(driver)
/// }
/// ```
pub trait LocalStreamDriver: Send + Sync + 'static {
    /// Polls the stable request until its worker completion is available.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context to wake when the request advances.
    /// * `request` - Stable identifier and owned command payload.
    fn poll_command(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
    ) -> Poll<io::Result<LocalStreamCompletion>>;

    /// Returns worker-confirmed write facts when this driver can establish
    /// them. Drivers without an observer retain the conservative unknown state.
    fn write_facts(&self) -> Option<(crate::LocalWriteFailureState, u64)> {
        None
    }

    /// Enqueues a close command without blocking when the owning handle drops.
    /// The command id must be idempotent with a later explicit `close` poll.
    /// # Parameters
    ///
    /// * `request_id` - Identifier assigned to the close command.
    fn request_close(&self, request_id: u64);
}
