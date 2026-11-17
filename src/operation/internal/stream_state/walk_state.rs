// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::PathBuf;
use std::task::Context;
use std::task::Poll;

use super::super::LOCAL_WALK_BATCH_BYTES;
use super::super::LOCAL_WALK_BATCH_ENTRIES;
use super::super::LocalWalkResource;
use super::SavedError;
use super::resource_limit;

/// Tracks one batched directory walk and its orderly stop/close sequence.
/// At most one request is pending, and the state retains close errors for later
/// polls after the underlying walker has stopped.
#[derive(Default)]
pub(crate) struct WalkState {
    /// Next nonzero id used to correlate a batch request with its reply.
    next_request_id: u64,
    /// Id of the sole batch request currently awaiting the worker.
    pending_request: Option<u64>,
    /// Whether the state has asked the walker to stop producing entries.
    stop_requested: bool,
    /// Whether the worker has acknowledged completion of close.
    closed: bool,
    /// Native close failure retained so later polls return the same result.
    close_error: Option<SavedError>,
}

impl WalkState {
    /// Polls one bounded batch, validating its byte and entry budgets before
    /// returning paths.
    ///
    /// # Parameters
    ///
    /// * `self` — walk state retaining the pending request and stop status.
    /// * `resource` — worker resource producing path batches.
    /// * `cx` — task context used to wait for the next batch.
    pub(in crate::operation::stream) fn poll_batch(
        &mut self,
        resource: &LocalWalkResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        if self.stop_requested {
            return match self.poll_close(resource, cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => Poll::Ready(Ok(None)),
            };
        }
        let request_id = *self.pending_request.get_or_insert_with(|| {
            self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
            self.next_request_id
        });
        match resource.poll_batch(cx, request_id) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                self.pending_request = None;
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(None)) => {
                self.pending_request = None;
                self.request_stop(resource);
                match self.poll_close(resource, cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(None)),
                }
            }
            Poll::Ready(Ok(Some(entries))) => {
                self.pending_request = None;
                let mut total = 0usize;
                for entry in &entries {
                    let bytes = entry.as_os_str().as_encoded_bytes().len();
                    if bytes > LOCAL_WALK_BATCH_BYTES {
                        self.request_stop(resource);
                        return Poll::Ready(Err(resource_limit("single walker path exceeds 256 KiB batch limit")));
                    }
                    total = match total.checked_add(bytes) {
                        Some(total) if total <= LOCAL_WALK_BATCH_BYTES => total,
                        _ => {
                            self.request_stop(resource);
                            return Poll::Ready(Err(resource_limit("walker batch exceeds 256 KiB path budget")));
                        }
                    };
                }
                if entries.len() > LOCAL_WALK_BATCH_ENTRIES {
                    self.request_stop(resource);
                    return Poll::Ready(Err(resource_limit("walker batch exceeds 128 entry limit")));
                }
                Poll::Ready(Ok(Some(entries)))
            }
        }
    }

    /// Requests stop exactly once so cancellation and end-of-walk share the
    /// same cleanup path.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    pub(in crate::operation::stream) fn request_stop(&mut self, resource: &LocalWalkResource) {
        if !self.stop_requested {
            self.stop_requested = true;
            resource.request_stop();
        }
    }

    /// Waits for the worker to close after stop has been requested and retains
    /// any close failure.
    ///
    /// # Parameters
    ///
    /// * `self` — state retaining stop and close completion status.
    /// * `resource` — worker resource that owns the directory walk.
    /// * `cx` — task context used while waiting for close completion.
    pub(in crate::operation::stream) fn poll_close(
        &mut self,
        resource: &LocalWalkResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.closed {
            return self.poll_closed_without_resource();
        }
        self.request_stop(resource);
        match resource.poll_closed(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.closed = result.is_ok();
                if let Err(error) = result {
                    self.close_error = Some(SavedError::capture(error));
                }
                self.poll_closed_without_resource()
            }
        }
    }

    /// Returns the stored close result without issuing another worker request.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    pub(in crate::operation::stream) fn poll_closed_without_resource(&self) -> Poll<io::Result<()>> {
        if let Some(error) = &self.close_error {
            Poll::Ready(Err(error.make()))
        } else if self.closed {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    /// Reports whether worker-side close completed successfully.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    pub(in crate::operation::stream) fn is_closed(&self) -> bool {
        self.closed
    }
}
