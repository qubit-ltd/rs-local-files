// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Cancellation-safe asynchronous local file streams.
//!
//! A [`LocalStreamResource`] is a control port. The native reader or writer
//! must be owned by its serial worker; implementations of [`LocalStreamDriver`]
//! only move owned requests to that worker and report their completion.

use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[path = "internal/stream_state.rs"]
mod stream_state;
use stream_state::WalkerResourceLimit;

#[path = "async_local_file_writer.rs"]
mod async_local_file_writer;
#[path = "local_native_stream.rs"]
mod local_native_stream;
pub use async_local_file_writer::AsyncLocalFileWriter;
#[path = "local_walk_close_observer.rs"]
mod local_walk_close_observer;
pub use local_walk_close_observer::LocalWalkCloseObserver;
#[path = "local_walk_resource.rs"]
mod local_walk_resource;
pub use local_walk_resource::LocalWalkResource;
#[path = "async_local_directory_walker.rs"]
mod async_local_directory_walker;
pub use async_local_directory_walker::AsyncLocalDirectoryWalker;
#[path = "local_walk_driver.rs"]
mod local_walk_driver;
pub use local_walk_driver::LocalWalkDriver;
#[path = "async_local_file_reader.rs"]
mod async_local_file_reader;
pub use async_local_file_reader::AsyncLocalFileReader;
#[path = "local_stream_command.rs"]
mod local_stream_command;
#[path = "local_stream_commit_error.rs"]
mod local_stream_commit_error;
#[path = "local_stream_completion.rs"]
mod local_stream_completion;
#[path = "local_stream_driver.rs"]
mod local_stream_driver;
#[path = "local_stream_request.rs"]
mod local_stream_request;
#[path = "local_stream_resource.rs"]
mod local_stream_resource;
pub use local_stream_resource::LocalStreamResource;
#[path = "worker_stream_inner.rs"]
mod worker_stream_inner;
use worker_stream_inner::WorkerStreamInner;
#[path = "worker_command.rs"]
mod worker_stream_command;
#[path = "worker_stream_state.rs"]
mod worker_stream_state;
#[path = "worker_walk_driver.rs"]
mod worker_walk_driver;
#[path = "worker_walk_inner.rs"]
mod worker_walk_inner;
pub(crate) use worker_walk_driver::WorkerWalkDriver;
#[path = "worker_stream_driver.rs"]
mod worker_stream_driver;
pub(crate) use worker_stream_driver::WorkerStreamDriver;
#[path = "worker_closable.rs"]
mod worker_closable;
use worker_closable::WorkerClosable;
#[path = "worker_cleanup.rs"]
mod worker_cleanup;
use worker_cleanup::WorkerCleanup;
#[path = "worker_walk_command.rs"]
mod worker_walk_command;
#[path = "worker_walk_cursor.rs"]
mod worker_walk_cursor;

pub(crate) use local_native_stream::LocalNativeStream;
pub use local_stream_command::LocalStreamCommand;
pub use local_stream_commit_error::LocalStreamCommitError;
pub use local_stream_completion::LocalStreamCompletion;
pub use local_stream_driver::LocalStreamDriver;
pub use local_stream_request::LocalStreamRequest;

/// Default owned read and write buffer size for one stream.
pub const LOCAL_STREAM_BUFFER_SIZE: usize = 64 * 1024;
/// Maximum directory entries returned by one walker batch.
pub const LOCAL_WALK_BATCH_ENTRIES: usize = 128;
/// Maximum encoded path bytes returned by one walker batch.
pub const LOCAL_WALK_BATCH_BYTES: usize = 256 * 1024;

/// Starts draining queued stream commands on the serial worker.
///
/// # Parameters
///
/// * `inner` - Shared queue, native handle, and worker state.
/// * `terminal` - Whether queue saturation may use reserved cleanup capacity.
fn start_stream_drain(inner: Arc<WorkerStreamInner>, terminal: bool) -> io::Result<()> {
    let task_inner = Arc::clone(&inner);
    let task = move || drain_stream_commands(task_inner);
    if inner.executor.enqueue_task(task).is_ok() {
        return Ok(());
    }

    if !terminal {
        inner.active.store(false, Ordering::Release);
        return Err(io::Error::new(io::ErrorKind::WouldBlock, "local worker queue is full"));
    }

    // Only terminal commands may use the cleanup lane reserved for this stream.
    let task_inner = Arc::clone(&inner);
    let mut fallback: Box<dyn FnOnce() + Send> = Box::new(move || drain_stream_commands(task_inner));
    for _ in 0..8 {
        let permit = inner
            .cleanup_permit
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .or_else(|| inner.executor.reserve_cleanup());
        let Some(permit) = permit else { break };
        match inner.executor.enqueue_cleanup(fallback, permit) {
            Ok(()) => return Ok(()),
            Err((job, permit)) => {
                *inner.cleanup_permit.lock().unwrap_or_else(|e| e.into_inner()) = Some(permit);
                fallback = job;
            }
        }
    }
    // Preserve the owner if all scheduling lanes are unavailable.
    inner.active.store(false, Ordering::Release);
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "local worker queue and cleanup lane are unavailable",
    ))
}

/// Completes queued requests with the error that stopped their worker drain.
///
/// # Parameters
///
/// * `inner` - Shared stream state whose pending requests must be failed.
/// * `error` - Worker scheduling or execution error to deliver to callers.
fn fail_stream_queue(inner: &WorkerStreamInner, error: io::Error) {
    let requests = {
        let mut queue = inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        inner.active.store(false, Ordering::Release);
        queue.drain(..).map(|request| request.id).collect::<Vec<_>>()
    };
    for id in requests {
        let waker = {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(command) = state.commands.get_mut(&id) else {
                continue;
            };
            command.result = Some(Err(io::Error::new(error.kind(), error.to_string())));
            command.waker.take()
        };
        if let Some(waker) = waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
        }
    }
}

/// Executes queued commands in FIFO order and wakes their polling callers.
///
/// # Parameters
///
/// * `inner` - Shared worker state retaining the native stream and request
///   queue.
fn drain_stream_commands(inner: Arc<WorkerStreamInner>) {
    loop {
        let request = {
            let mut queue = inner.queue.lock().unwrap_or_else(|e| e.into_inner());
            match queue.pop_front() {
                Some(request) => request,
                None => {
                    inner.active.store(false, Ordering::Release);
                    return;
                }
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut native = inner.native.lock().unwrap_or_else(|e| e.into_inner());
            execute_stream_command(&mut native, &request.command)
        }))
        .unwrap_or_else(|_| Err(io::Error::other("Unconfirmed: local stream worker panicked")));
        if let Ok(completion) = &result {
            match completion {
                LocalStreamCompletion::Written(bytes) => {
                    let previous = inner
                        .written_bytes
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                            Some(current.saturating_add(*bytes as u64))
                        })
                        .unwrap_or_else(|current| current);
                    let total = previous.saturating_add(*bytes as u64);
                    if inner.append_mode
                        && *bytes > 0
                        && let Some(observer) = &inner.observer
                    {
                        record_stream_fact(
                            observer,
                            super::LocalPublicationState::Published,
                            super::LocalDurabilityState::Unknown,
                            total,
                        );
                    }
                }
                LocalStreamCompletion::Committed(outcome) => {
                    if let Some(observer) = &inner.observer {
                        record_stream_fact(
                            observer,
                            super::LocalPublicationState::Published,
                            if outcome.durable() {
                                super::LocalDurabilityState::Durable
                            } else {
                                super::LocalDurabilityState::Unknown
                            },
                            inner.written_bytes.load(Ordering::Acquire),
                        );
                    }
                }
                LocalStreamCompletion::CommitFailed { error, .. } => {
                    if let Some(observer) = &inner.observer {
                        let publication = match error.state() {
                            crate::LocalWriteFailureState::NotPublished => super::LocalPublicationState::NotPublished,
                            crate::LocalWriteFailureState::Published => super::LocalPublicationState::Published,
                            crate::LocalWriteFailureState::Indeterminate => super::LocalPublicationState::Unknown,
                        };
                        record_stream_fact(
                            observer,
                            publication,
                            if publication == super::LocalPublicationState::Published {
                                super::LocalDurabilityState::Failed
                            } else {
                                super::LocalDurabilityState::Unknown
                            },
                            inner.written_bytes.load(Ordering::Acquire),
                        );
                    }
                }
                _ => {}
            }
        }
        let waker = {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(slot) = state.commands.get_mut(&request.id) else {
                continue;
            };
            slot.result = Some(result);
            slot.waker.take()
        };
        if let Some(waker) = waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
        }
    }
}

/// Updates the operation observer after the worker confirms append publication.
///
/// # Parameters
///
/// * `observer` - Operation facts associated with the append submission.
/// * `publication` - Publication state confirmed by the worker.
/// * `durability` - Durability state confirmed by the worker.
/// * `modified_bytes` - Maximum byte count confirmed written so far.
fn record_stream_fact(
    observer: &super::LocalFactObserver,
    publication: super::LocalPublicationState,
    durability: super::LocalDurabilityState,
    modified_bytes: u64,
) {
    let mut snapshot = observer.snapshot();
    if snapshot.publication != super::LocalPublicationState::Published {
        snapshot.publication = publication;
    }
    if snapshot.publication == super::LocalPublicationState::Published {
        snapshot.modified_count = 1;
        snapshot.modified_bytes = snapshot.modified_bytes.max(modified_bytes);
        if durability != super::LocalDurabilityState::Unknown {
            snapshot.durability = durability;
        }
    }
    observer.update(snapshot);
}

/// Applies one validated command to the worker-owned native stream.
///
/// # Parameters
///
/// * `native` - Optional native reader or writer retained by the worker.
/// * `command` - Stable operation request to execute.
fn execute_stream_command(
    native: &mut Option<LocalNativeStream>,
    command: &LocalStreamCommand,
) -> io::Result<LocalStreamCompletion> {
    if matches!(command, LocalStreamCommand::Commit) {
        let Some(LocalNativeStream::Writer(writer)) = native.take() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "commit requires a writer"));
        };
        return match writer.commit() {
            Ok(outcome) => Ok(LocalStreamCompletion::Committed(outcome)),
            Err(error) => {
                let (file_error, state, retry_writer) = error.into_parts();
                let retryable = retry_writer.is_some();
                if let Some(writer) = retry_writer {
                    *native = Some(LocalNativeStream::Writer(Box::new(writer)));
                }
                Ok(LocalStreamCompletion::CommitFailed {
                    error: crate::LocalFileCommitError::new(file_error, state, None),
                    retryable,
                })
            }
        };
    }
    if matches!(command, LocalStreamCommand::Close | LocalStreamCommand::Abort) {
        let Some(previous) = native.take() else {
            return Ok(LocalStreamCompletion::Closed);
        };
        drop(previous);
        return Ok(LocalStreamCompletion::Closed);
    }
    let Some(native) = native.as_mut() else {
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "local stream is closed"));
    };
    match (native, command) {
        (LocalNativeStream::Reader(reader), LocalStreamCommand::Read { max_bytes }) => {
            let mut bytes = vec![0; (*max_bytes).min(LOCAL_STREAM_BUFFER_SIZE)];
            let count = qubit_io::Input::read(reader, &mut bytes)?;
            bytes.truncate(count);
            Ok(LocalStreamCompletion::Read(bytes))
        }
        (LocalNativeStream::Writer(writer), LocalStreamCommand::Write { bytes }) => {
            let count = qubit_io::Output::write(writer.as_mut(), bytes)?;
            Ok(LocalStreamCompletion::Written(count))
        }
        (LocalNativeStream::Writer(writer), LocalStreamCommand::Flush) => {
            qubit_io::Output::flush(writer.as_mut())?;
            Ok(LocalStreamCompletion::Flushed)
        }
        (LocalNativeStream::Reader(reader), LocalStreamCommand::Seek(position)) => {
            Ok(LocalStreamCompletion::Seeked(std::io::Seek::seek(reader, *position)?))
        }
        (LocalNativeStream::Writer(_), LocalStreamCommand::Seek(_)) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "seeking a local file writer is unsupported",
        )),
        (LocalNativeStream::Reader(_), LocalStreamCommand::Write { .. })
        | (LocalNativeStream::Reader(_), LocalStreamCommand::Flush)
        | (LocalNativeStream::Reader(_), LocalStreamCommand::Commit)
        | (LocalNativeStream::Writer(_), LocalStreamCommand::Commit)
        | (LocalNativeStream::Writer(_), LocalStreamCommand::Read { .. }) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stream command has the wrong direction",
        )),
        (_, LocalStreamCommand::Close | LocalStreamCommand::Abort) => {
            unreachable!("terminal command handled above")
        }
    }
}

/// Maps worker and resource-limit errors into the local filesystem error model.
///
/// # Parameters
///
/// * `error` - Error returned by a directory worker or close observer.
pub(super) fn map_walk_error(error: io::Error) -> crate::LocalFileError {
    if let Some(limit) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<WalkerResourceLimit>())
    {
        crate::error::LocalFileError::new(
            crate::error::LocalFileErrorKind::ResourceLimit,
            crate::error::LocalFileOperation::List,
        )
        .with_reason("asynchronous walker batch resource limit exceeded")
        .with_detail(limit.to_string())
    } else {
        crate::error::LocalFileError::from_io(crate::error::LocalFileOperation::List, None, None, error)
    }
}
