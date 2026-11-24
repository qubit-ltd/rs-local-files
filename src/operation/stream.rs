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
#[cfg(test)]
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[cfg(test)]
static STREAM_WORKER_PANIC_PAYLOAD_DROPS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static WALKER_WORKER_PANIC_PAYLOAD_DROPS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
struct StreamWorkerPanicPayload;
#[cfg(test)]
struct WalkerWorkerPanicPayload;

#[cfg(test)]
impl Drop for StreamWorkerPanicPayload {
    fn drop(&mut self) {
        STREAM_WORKER_PANIC_PAYLOAD_DROPS.fetch_add(1, Ordering::SeqCst);
        panic!("stream worker panic payload destructor panicked");
    }
}

#[cfg(test)]
impl Drop for WalkerWorkerPanicPayload {
    fn drop(&mut self) {
        WALKER_WORKER_PANIC_PAYLOAD_DROPS.fetch_add(1, Ordering::SeqCst);
        panic!("walker worker panic payload destructor panicked");
    }
}

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
#[path = "native_stream_writer.rs"]
mod native_stream_writer;
#[path = "worker_stream_inner.rs"]
mod worker_stream_inner;
#[path = "worker_stream_queued_request.rs"]
mod worker_stream_queued_request;
use worker_stream_inner::WorkerStreamInner;
#[path = "close_wait_registration.rs"]
mod close_wait_registration;
#[path = "worker_command.rs"]
mod worker_stream_command;
#[path = "worker_stream_state.rs"]
mod worker_stream_state;
#[path = "worker_walk_close_state.rs"]
mod worker_walk_close_state;
#[path = "worker_walk_driver.rs"]
mod worker_walk_driver;
#[path = "worker_walk_inner.rs"]
mod worker_walk_inner;
pub(crate) use worker_walk_driver::WorkerWalkDriver;
pub(super) use worker_walk_inner::dispose_panic_payload;

/// Clones a caller waker while retaining any panic payload for the poller's
/// existing failure boundary.
///
/// # Parameters
///
/// * `waker` - The caller's task waker to clone.
pub(super) fn clone_poll_waker(waker: &std::task::Waker) -> Result<std::task::Waker, Box<dyn std::any::Any + Send>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.clone()))
}

#[cfg(test)]
pub(super) fn panic_clone_waker() -> std::task::Waker {
    unsafe fn panic_clone(_: *const ()) -> std::task::RawWaker {
        panic!("injected task waker clone panic");
    }
    unsafe fn no_op(_: *const ()) {}
    static VTABLE: std::task::RawWakerVTable = std::task::RawWakerVTable::new(panic_clone, no_op, no_op, no_op);
    // SAFETY: the vtable never dereferences the null data pointer.
    unsafe { std::task::Waker::from_raw(std::task::RawWaker::new(std::ptr::null(), &VTABLE)) }
}
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
        queue.drain(..).map(|queued| queued.request.id).collect::<Vec<_>>()
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
        if let Some(waker) = waker
            && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()))
        {
            dispose_panic_payload(payload);
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
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut native = inner.native.lock().unwrap_or_else(|e| e.into_inner());
            #[cfg(test)]
            if inner.panic_next_command.swap(false, Ordering::SeqCst) {
                std::panic::panic_any(StreamWorkerPanicPayload);
            }
            execute_stream_command_at(&mut native, &request.request.command, request.write_offset)
        })) {
            Ok(result) => result,
            Err(payload) => {
                dispose_panic_payload(payload);
                Err(io::Error::other("Unconfirmed: local stream worker panicked"))
            }
        };
        if matches!(
            request.request.command,
            LocalStreamCommand::Close | LocalStreamCommand::Abort
        ) && matches!(&result, Ok(LocalStreamCompletion::Closed))
            && inner.native.lock().unwrap_or_else(|e| e.into_inner()).is_none()
        {
            inner.release_handle_budget();
            inner.release_buffer_budget();
        }
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
                    if let Some(observer) = &inner.observer
                        && (!inner.append_mode || inner.written_bytes.load(Ordering::Acquire) > 0)
                    {
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
            let Some(slot) = state.commands.get_mut(&request.request.id) else {
                continue;
            };
            slot.result = Some(result);
            slot.waker.take()
        };
        if let Some(waker) = waker
            && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()))
        {
            dispose_panic_payload(payload);
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
#[cfg(test)]
fn execute_stream_command(
    native: &mut Option<LocalNativeStream>,
    command: &LocalStreamCommand,
) -> io::Result<LocalStreamCompletion> {
    execute_stream_command_at(native, command, 0)
}

/// Executes one command, borrowing the remaining write payload when applicable.
///
/// # Parameters
///
/// * `native` - Native stream retained by this serial worker.
/// * `command` - Owned request command to execute.
/// * `write_offset` - Number of payload bytes already written.
fn execute_stream_command_at(
    native: &mut Option<LocalNativeStream>,
    command: &LocalStreamCommand,
    write_offset: usize,
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
        let Some(previous) = native.as_mut() else {
            return Ok(LocalStreamCompletion::Closed);
        };
        if let LocalNativeStream::Writer(writer) = previous {
            let _ = writer.abort().map_err(crate::LocalFileError::into_io_error)?;
        }
        native.take();
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
            let remaining = bytes
                .get(write_offset..)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "native write offset exceeds payload"))?;
            let count = qubit_io::Output::write(writer.as_mut(), remaining)?;
            #[cfg(feature = "test-support")]
            crate::test_support::hit_test_gate("rlf9-writer-after-stage-write");
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
    if error
        .get_ref()
        .is_some_and(|source| source.is::<crate::LocalFileError>())
    {
        return *error
            .into_inner()
            .expect("LocalFileError source was checked above")
            .downcast::<crate::LocalFileError>()
            .expect("LocalFileError source type was checked above");
    }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::fs::File;
    use std::io::SeekFrom;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

    use super::LocalNativeStream;
    use super::LocalStreamCommand;
    use super::LocalStreamCompletion;
    use super::drain_stream_commands;
    use super::execute_stream_command;
    use super::fail_stream_queue;
    use super::map_walk_error;
    use super::record_stream_fact;
    use super::stream_state::resource_limit;
    use super::worker_stream_command::WorkerCommand;
    use super::worker_stream_inner::WorkerStreamInner;
    use super::worker_stream_state::WorkerStreamState;
    use crate::operation::LocalDurabilityState;
    use crate::operation::LocalFactObserver;
    use crate::operation::LocalPublicationState;

    fn temp_path(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("qubit-local-stream-{label}-{}-{nonce}", std::process::id()))
    }

    struct PanicOnDrop;

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("panic payload destructor panicked");
        }
    }

    struct PanicPayloadWake;

    impl Wake for PanicPayloadWake {
        fn wake(self: std::sync::Arc<Self>) {
            std::panic::panic_any(PanicOnDrop);
        }
    }

    #[test]
    fn test_reader_commands_preserve_cursor_and_reject_writer_only_operations() {
        let path = temp_path("reader");
        std::fs::write(&path, b"stream bytes").expect("write fixture");
        let reader =
            crate::LocalFileReader::from_file(File::open(&path).expect("open fixture")).expect("construct reader");
        let mut native = Some(LocalNativeStream::Reader(reader));

        assert!(matches!(
            execute_stream_command(&mut native, &LocalStreamCommand::Read { max_bytes: 6 }).unwrap(),
            LocalStreamCompletion::Read(bytes) if bytes == b"stream"
        ));
        assert!(matches!(
            execute_stream_command(&mut native, &LocalStreamCommand::Seek(SeekFrom::Start(7))).unwrap(),
            LocalStreamCompletion::Seeked(7)
        ));
        assert!(matches!(
            execute_stream_command(&mut native, &LocalStreamCommand::Read { max_bytes: 99 }).unwrap(),
            LocalStreamCompletion::Read(bytes) if bytes == b"bytes"
        ));
        assert_eq!(
            execute_stream_command(&mut native, &LocalStreamCommand::Flush)
                .expect_err("reader cannot flush")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            execute_stream_command(
                &mut native,
                &LocalStreamCommand::Write {
                    bytes: std::sync::Arc::from(&b"x"[..])
                }
            )
            .expect_err("reader cannot write")
            .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(matches!(
            execute_stream_command(&mut native, &LocalStreamCommand::Close).unwrap(),
            LocalStreamCompletion::Closed
        ));
        assert!(matches!(
            execute_stream_command(&mut native, &LocalStreamCommand::Abort).unwrap(),
            LocalStreamCompletion::Closed
        ));
        assert_eq!(
            execute_stream_command(&mut native, &LocalStreamCommand::Read { max_bytes: 1 })
                .expect_err("closed stream rejects reads")
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
        std::fs::remove_file(path).expect("remove fixture");
    }

    #[test]
    fn test_append_writer_commands_write_flush_commit_and_reject_reads_and_seek() {
        let path = temp_path("writer");
        let file = File::create(&path).expect("create fixture");
        let backend = crate::writer::internal::LocalFileWriterBackend::Append(file);
        let writer = crate::LocalFileWriter::new(path.clone(), backend, crate::LocalWriteOptions::default());
        let mut native = Some(LocalNativeStream::Writer(Box::new(writer)));

        assert!(matches!(
            execute_stream_command(
                &mut native,
                &LocalStreamCommand::Write {
                    bytes: std::sync::Arc::from(&b"append"[..])
                }
            )
            .unwrap(),
            LocalStreamCompletion::Written(6)
        ));
        assert!(matches!(
            execute_stream_command(&mut native, &LocalStreamCommand::Flush).unwrap(),
            LocalStreamCompletion::Flushed
        ));
        assert_eq!(
            execute_stream_command(&mut native, &LocalStreamCommand::Seek(SeekFrom::Start(0)))
                .expect_err("writer cannot seek")
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            execute_stream_command(&mut native, &LocalStreamCommand::Read { max_bytes: 1 })
                .expect_err("writer cannot read")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(matches!(
            execute_stream_command(&mut native, &LocalStreamCommand::Commit).unwrap(),
            LocalStreamCompletion::Committed(_)
        ));
        assert!(native.is_none());
        assert_eq!(std::fs::read(&path).expect("read committed fixture"), b"append");
        std::fs::remove_file(path).expect("remove fixture");
    }

    #[test]
    fn test_commit_and_close_are_safe_when_the_native_stream_is_missing_or_wrong_kind() {
        let mut missing = None;
        assert_eq!(
            execute_stream_command(&mut missing, &LocalStreamCommand::Commit)
                .expect_err("commit requires a writer")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(matches!(
            execute_stream_command(&mut missing, &LocalStreamCommand::Close).unwrap(),
            LocalStreamCompletion::Closed
        ));

        let mut reader = None;
        reader.replace(LocalNativeStream::Reader(
            crate::LocalFileReader::from_file(File::open("Cargo.toml").expect("open package manifest"))
                .expect("construct reader"),
        ));
        assert_eq!(
            execute_stream_command(&mut reader, &LocalStreamCommand::Commit)
                .expect_err("commit cannot consume a reader")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(reader.is_none());
    }

    #[test]
    fn test_walk_error_mapping_preserves_resource_limits_and_regular_io_context() {
        let error = map_walk_error(resource_limit("batch exceeds configured limit"));
        assert_eq!(error.kind(), crate::LocalFileErrorKind::ResourceLimit);
        assert!(error.reason().unwrap().contains("resource limit"));

        let error = map_walk_error(std::io::Error::new(std::io::ErrorKind::NotFound, "missing entry"));
        assert_eq!(error.kind(), crate::LocalFileErrorKind::NotFound);
        assert_eq!(error.operation(), crate::LocalFileOperation::List);
        assert!(error.to_string().contains("missing entry"));
    }

    #[test]
    fn test_record_stream_fact_preserves_publication_and_maximum_confirmed_bytes() {
        let observer = LocalFactObserver::new();
        record_stream_fact(
            &observer,
            LocalPublicationState::Published,
            LocalDurabilityState::Unknown,
            8,
        );
        record_stream_fact(
            &observer,
            LocalPublicationState::Unknown,
            LocalDurabilityState::Failed,
            3,
        );

        let snapshot = observer.snapshot();
        assert_eq!(snapshot.publication, LocalPublicationState::Published);
        assert_eq!(snapshot.durability, LocalDurabilityState::Failed);
        assert_eq!(snapshot.modified_count, 1);
        assert_eq!(snapshot.modified_bytes, 8);
    }

    #[test]
    fn test_worker_drain_records_append_write_and_completes_request() {
        let path = temp_path("drain-append");
        let manager = qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).unwrap();
        let executor = super::super::LocalIoExecutor::new(manager, crate::LocalExecutionConfig::default()).unwrap();
        let file = File::create(&path).expect("create append fixture");
        let backend = crate::writer::internal::LocalFileWriterBackend::Append(file);
        let writer = crate::LocalFileWriter::new(path.clone(), backend, crate::LocalWriteOptions::default());
        let observer = LocalFactObserver::new();
        let inner = std::sync::Arc::new(WorkerStreamInner {
            executor: executor.clone(),
            native: Mutex::new(Some(LocalNativeStream::Writer(Box::new(writer)))),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::from([
                    (
                        41,
                        WorkerCommand {
                            result: None,
                            waker: Some(Waker::from(std::sync::Arc::new(PanicPayloadWake))),
                        },
                    ),
                    (
                        42,
                        WorkerCommand {
                            result: None,
                            waker: None,
                        },
                    ),
                ]),
            }),
            queue: Mutex::new(VecDeque::from([
                super::worker_stream_queued_request::WorkerStreamQueuedRequest {
                    request: super::LocalStreamRequest {
                        id: 41,
                        command: LocalStreamCommand::Write {
                            bytes: std::sync::Arc::from(&b"append through worker"[..]),
                        },
                    },
                    write_offset: 0,
                },
                super::worker_stream_queued_request::WorkerStreamQueuedRequest {
                    request: super::LocalStreamRequest {
                        id: 42,
                        command: LocalStreamCommand::Write {
                            bytes: std::sync::Arc::from(&b"!"[..]),
                        },
                    },
                    write_offset: 0,
                },
            ])),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: std::sync::atomic::AtomicBool::new(true),
            cleanup_permit: Mutex::new(Some(executor.reserve_cleanup().unwrap())),
            _buffer_budget: std::sync::Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            _handle_budget: Mutex::new(Some(std::sync::Arc::new(executor.reserve_handle().unwrap()))),
            observer: Some(observer.clone()),
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: true,
            #[cfg(test)]
            panic_next_command: super::AtomicBool::new(false),
        });

        #[cfg(panic = "unwind")]
        {
            use crate::operation::poison_test_mutex;

            poison_test_mutex(&inner.queue);
            poison_test_mutex(&inner.state);
            poison_test_mutex(&inner.native);
        }
        drain_stream_commands(std::sync::Arc::clone(&inner));

        assert!(!inner.active.load(Ordering::Acquire));
        assert!(
            inner
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert_eq!(inner.written_bytes.load(Ordering::Acquire), 22);
        let mut state = inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(matches!(
            state.commands.get_mut(&41).unwrap().result.take().unwrap(),
            Ok(LocalStreamCompletion::Written(21))
        ));
        assert!(matches!(
            state.commands.get_mut(&42).unwrap().result.take().unwrap(),
            Ok(LocalStreamCompletion::Written(1))
        ));
        drop(state);

        let driver = super::WorkerStreamDriver::from_inner(std::sync::Arc::clone(&inner));
        let request = super::LocalStreamRequest {
            id: 43,
            command: LocalStreamCommand::Write {
                bytes: std::sync::Arc::from(&b" again"[..]),
            },
        };
        let mut context = std::task::Context::from_waker(Waker::noop());
        assert!(matches!(
            super::LocalStreamDriver::poll_command(&driver, &mut context, &request),
            std::task::Poll::Pending
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let result = loop {
            if let std::task::Poll::Ready(result) =
                super::LocalStreamDriver::poll_command(&driver, &mut context, &request)
            {
                break result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "rescheduled stream command did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(matches!(result, Ok(LocalStreamCompletion::Written(6))));
        assert!(!inner.active.load(Ordering::Acquire));

        let snapshot = observer.snapshot();
        assert_eq!(snapshot.publication, LocalPublicationState::Published);
        assert_eq!(snapshot.modified_count, 1);
        assert_eq!(snapshot.modified_bytes, 28);
        assert_eq!(inner.written_bytes.load(Ordering::Acquire), 28);
        assert_eq!(std::fs::read(&path).unwrap(), b"append through worker! again");
        std::fs::remove_file(path).expect("remove fixture");
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_stream_worker_delivers_panic_error_and_processes_later_requests() {
        use std::time::Duration;

        let manager = qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).unwrap();
        let executor = super::super::LocalIoExecutor::new(manager, crate::LocalExecutionConfig::default()).unwrap();
        let inner = std::sync::Arc::new(WorkerStreamInner {
            executor: executor.clone(),
            native: Mutex::new(None),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::from([
                    (
                        41,
                        WorkerCommand {
                            result: None,
                            waker: None,
                        },
                    ),
                    (
                        42,
                        WorkerCommand {
                            result: None,
                            waker: None,
                        },
                    ),
                ]),
            }),
            queue: Mutex::new(VecDeque::from([
                super::worker_stream_queued_request::WorkerStreamQueuedRequest {
                    request: super::LocalStreamRequest {
                        id: 41,
                        command: LocalStreamCommand::Close,
                    },
                    write_offset: 0,
                },
                super::worker_stream_queued_request::WorkerStreamQueuedRequest {
                    request: super::LocalStreamRequest {
                        id: 42,
                        command: LocalStreamCommand::Close,
                    },
                    write_offset: 0,
                },
            ])),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: std::sync::atomic::AtomicBool::new(true),
            cleanup_permit: Mutex::new(Some(executor.reserve_cleanup().unwrap())),
            _buffer_budget: std::sync::Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            _handle_budget: Mutex::new(Some(std::sync::Arc::new(executor.reserve_handle().unwrap()))),
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
            #[cfg(test)]
            panic_next_command: super::AtomicBool::new(false),
        });
        super::STREAM_WORKER_PANIC_PAYLOAD_DROPS.store(0, Ordering::SeqCst);
        inner.panic_next_command.store(true, Ordering::SeqCst);

        // A separate worker can finish a command without consuming this worker's
        // injection.
        let unrelated = std::sync::Arc::new(WorkerStreamInner {
            executor: executor.clone(),
            native: Mutex::new(None),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::from([(
                    51,
                    WorkerCommand {
                        result: None,
                        waker: None,
                    },
                )]),
            }),
            queue: Mutex::new(VecDeque::from([
                super::worker_stream_queued_request::WorkerStreamQueuedRequest {
                    request: super::LocalStreamRequest {
                        id: 51,
                        command: LocalStreamCommand::Close,
                    },
                    write_offset: 0,
                },
            ])),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: std::sync::atomic::AtomicBool::new(true),
            cleanup_permit: Mutex::new(Some(executor.reserve_cleanup().unwrap())),
            _buffer_budget: std::sync::Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            _handle_budget: Mutex::new(Some(std::sync::Arc::new(executor.reserve_handle().unwrap()))),
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
            #[cfg(test)]
            panic_next_command: super::AtomicBool::new(false),
        });
        drain_stream_commands(std::sync::Arc::clone(&unrelated));
        assert!(matches!(
            unrelated.state.lock().unwrap().commands[&51].result,
            Some(Ok(LocalStreamCompletion::Closed))
        ));
        assert!(inner.panic_next_command.load(Ordering::SeqCst));
        assert_eq!(0, super::STREAM_WORKER_PANIC_PAYLOAD_DROPS.load(Ordering::SeqCst));

        let drain = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drain_stream_commands(std::sync::Arc::clone(&inner));
        }));
        assert!(drain.is_ok(), "worker panic payload disposal must not unwind the drain");
        assert_eq!(1, super::STREAM_WORKER_PANIC_PAYLOAD_DROPS.load(Ordering::SeqCst));
        assert_eq!(
            "Unconfirmed: local stream worker panicked",
            inner.state.lock().unwrap().commands[&41]
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap_err()
                .to_string()
        );
        assert!(matches!(
            inner.state.lock().unwrap().commands[&42].result,
            Some(Ok(LocalStreamCompletion::Closed))
        ));
        assert!(!inner.active.load(Ordering::Acquire));

        let driver = super::WorkerStreamDriver::from_inner(std::sync::Arc::clone(&inner));
        let request = super::LocalStreamRequest {
            id: 43,
            command: LocalStreamCommand::Close,
        };
        let mut context = std::task::Context::from_waker(Waker::noop());
        assert!(super::LocalStreamDriver::poll_command(&driver, &mut context, &request).is_pending());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let std::task::Poll::Ready(result) =
                super::LocalStreamDriver::poll_command(&driver, &mut context, &request)
            {
                assert!(matches!(result, Ok(LocalStreamCompletion::Closed)));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "re-enqueued stream command did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!inner.active.load(Ordering::Acquire));
    }

    #[test]
    fn test_fail_stream_queue_fails_queued_commands_and_wakes_callers() {
        struct CountWake(AtomicUsize);
        impl Wake for CountWake {
            fn wake(self: std::sync::Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let manager = qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).unwrap();
        let executor = super::super::LocalIoExecutor::new(manager, crate::LocalExecutionConfig::default()).unwrap();
        let mut commands = HashMap::new();
        let wake = std::sync::Arc::new(CountWake(AtomicUsize::new(0)));
        commands.insert(
            41,
            WorkerCommand {
                result: None,
                waker: Some(Waker::from(std::sync::Arc::new(PanicPayloadWake))),
            },
        );
        commands.insert(
            42,
            WorkerCommand {
                result: None,
                waker: Some(Waker::from(std::sync::Arc::clone(&wake))),
            },
        );
        let inner = WorkerStreamInner {
            executor: executor.clone(),
            native: Mutex::new(None),
            state: Mutex::new(WorkerStreamState { commands }),
            queue: Mutex::new(VecDeque::from([
                super::worker_stream_queued_request::WorkerStreamQueuedRequest {
                    request: super::LocalStreamRequest {
                        id: 41,
                        command: LocalStreamCommand::Close,
                    },
                    write_offset: 0,
                },
                super::worker_stream_queued_request::WorkerStreamQueuedRequest {
                    request: super::LocalStreamRequest {
                        id: 42,
                        command: LocalStreamCommand::Abort,
                    },
                    write_offset: 0,
                },
            ])),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: std::sync::atomic::AtomicBool::new(true),
            cleanup_permit: Mutex::new(Some(executor.reserve_cleanup().unwrap())),
            _buffer_budget: std::sync::Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            _handle_budget: Mutex::new(Some(std::sync::Arc::new(executor.reserve_handle().unwrap()))),
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
            #[cfg(test)]
            panic_next_command: super::AtomicBool::new(false),
        };

        #[cfg(panic = "unwind")]
        {
            crate::operation::poison_test_mutex(&inner.queue);
            crate::operation::poison_test_mutex(&inner.state);
        }
        fail_stream_queue(
            &inner,
            std::io::Error::new(std::io::ErrorKind::WouldBlock, "queue full"),
        );

        assert!(!inner.active.load(Ordering::Acquire));
        assert!(
            inner
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        let state = inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for id in [41, 42] {
            assert_eq!(
                state.commands[&id]
                    .result
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);
    }
}
