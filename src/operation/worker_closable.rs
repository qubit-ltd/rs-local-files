// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

use super::LocalStreamCommand;
use super::LocalStreamCompletion;
use super::LocalStreamRequest;
use super::LocalStreamResource;
use super::LocalWalkResource;

/// Contract for resources that release native handles through their worker.
pub(super) trait WorkerClosable: Send + Sync + 'static {
    /// Enqueues idempotent native cleanup on the owning serial worker.
    fn request_cleanup(&self);
    /// Waits until the worker confirms cleanup and releases the native handle.
    fn poll_cleanup(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
    /// Returns the operation resource identifier associated with this port.
    fn resource_id(&self) -> Arc<Mutex<Option<qubit_operation::ResourceId>>>;
}

impl WorkerClosable for LocalStreamResource {
    /// Requests abort cleanup from the worker.
    fn request_cleanup(&self) {
        self.request_close(0);
    }

    /// Polls the terminal command until native closure or a worker error.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used by the command driver for wakeups.
    fn poll_cleanup(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let request = LocalStreamRequest {
            id: 0,
            command: LocalStreamCommand::Abort,
        };
        match self.poll_command(cx, &request) {
            Poll::Ready(Ok(LocalStreamCompletion::Closed)) => Poll::Ready(Ok(())),
            Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                Poll::Ready(Err(io::Error::other(format!(
                    "Unconfirmed {operation}: {detail}"
                ))))
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected stream cleanup completion",
            ))),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }

    /// Returns the resource identity recorded when the scope adopted the port.
    fn resource_id(&self) -> Arc<Mutex<Option<qubit_operation::ResourceId>>> {
        Arc::clone(&self.resource_id)
    }
}

impl WorkerClosable for LocalWalkResource {
    /// Requests iterator stop and native handle closure.
    fn request_cleanup(&self) {
        self.request_stop();
    }
    /// Polls for confirmed native iterator closure.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context used by the worker driver for wakeups.
    fn poll_cleanup(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_closed(cx)
    }
    /// Returns the resource identity recorded when the scope adopted the port.
    fn resource_id(&self) -> Arc<Mutex<Option<qubit_operation::ResourceId>>> {
        Arc::clone(&self.resource_id)
    }
}
