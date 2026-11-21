// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::PathBuf;
use std::task::Poll;

use qubit_operation::ResourceRef;

use super::LocalWalkCloseObserver;
use super::LocalWalkResource;
use super::map_walk_error;
use super::stream_state::WalkState;

/// Asynchronous directory walker that returns bounded path batches.
///
/// The filesystem operation that opens a walker supplies this handle. Each
/// batch is owned and bounded; `close` waits for native handle confirmation.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::AsyncLocalDirectoryWalker;
/// use qubit_local_files::LocalResult;
/// use std::path::PathBuf;
///
/// async fn next(walker: &mut AsyncLocalDirectoryWalker)
///     -> LocalResult<Option<Vec<PathBuf>>>
/// {
///     walker.next_batch().await
/// }
/// ```
pub struct AsyncLocalDirectoryWalker {
    /// Managed worker port retained until traversal or close completes.
    resource: Option<ResourceRef<LocalWalkResource>>,
    /// Request and stop state preserved when an async future is cancelled.
    state: WalkState,
}

impl AsyncLocalDirectoryWalker {
    /// Creates an exclusive walker over a managed worker resource.
    ///
    /// # Parameters
    ///
    /// * `resource` - Managed worker port with exclusive iterator ownership.
    pub fn new(resource: ResourceRef<LocalWalkResource>) -> io::Result<Self> {
        resource.claim()?;
        Ok(Self {
            resource: Some(resource),
            state: WalkState::default(),
        })
    }

    /// Returns the next bounded batch, or `None` after EOF and confirmed close.
    /// If an iterator error follows paths already collected for a batch, those
    /// paths are returned first and the error is returned by the next call.
    pub async fn next_batch(&mut self) -> crate::LocalResult<Option<Vec<PathBuf>>> {
        if self.state.is_closed() {
            return Ok(None);
        }
        let Some(resource) = self.resource.as_deref() else {
            return match self.state.poll_closed_without_resource() {
                Poll::Ready(Ok(())) => Ok(None),
                Poll::Ready(Err(error)) => Err(map_walk_error(error)),
                Poll::Pending => unreachable!("a released walker has completed closure"),
            };
        };
        let mut registration = resource.close_registration();
        let result = {
            let state = &mut self.state;
            std::future::poll_fn(move |cx| state.poll_batch(resource, cx, &mut registration)).await
        };
        if result.as_ref().is_ok_and(Option::is_none) && self.state.is_closed() {
            self.resource.take();
        }
        result.map_err(map_walk_error)
    }

    /// Stops traversal and waits for the worker to confirm handle closure.
    ///
    /// If worker cleanup fails, this result preserves that close failure.
    /// Operation settlement may retain a recovery handle for the worker owner;
    /// keep that handle through retry or operator handoff. Retrying cannot
    /// restart an executor whose worker has stopped.
    pub async fn close(&mut self) -> crate::LocalResult<()> {
        let Some(resource) = self.resource.as_deref() else {
            return match self.state.poll_closed_without_resource() {
                Poll::Ready(result) => result.map_err(map_walk_error),
                Poll::Pending => unreachable!("a released walker has completed closure"),
            };
        };
        let mut registration = resource.close_registration();
        self.state.request_stop(resource);
        let result = {
            let state = &mut self.state;
            std::future::poll_fn(move |cx| state.poll_close(resource, cx, &mut registration)).await
        };
        if self.state.is_closed() {
            self.resource.take();
        }
        result.map_err(map_walk_error)
    }

    /// Returns a close observer that survives dropping this walker handle.
    ///
    /// An observer created after close confirmation completes immediately and
    /// does not retain the worker resource. An observer created before close
    /// confirmation retains that resource and continues observing the worker
    /// after this walker is dropped. Release such an observer's resource
    /// reference before waiting for operation settlement.
    pub fn close_observer(&self) -> LocalWalkCloseObserver {
        if self.state.is_closed() {
            LocalWalkCloseObserver::closed()
        } else if let Some(resource) = &self.resource {
            LocalWalkCloseObserver::observing(resource.clone())
        } else {
            LocalWalkCloseObserver::closed()
        }
    }
}

impl Drop for AsyncLocalDirectoryWalker {
    /// Requests native iterator closure when traversal is abandoned.
    fn drop(&mut self) {
        if !self.state.is_closed()
            && let Some(resource) = &self.resource
        {
            self.state.request_stop(resource);
        }
    }
}
