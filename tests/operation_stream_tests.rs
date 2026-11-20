// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "async")]

use std::collections::HashMap;
use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::sync_channel;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;

use qubit_io::AsyncInput;
use qubit_io::AsyncOutput;
use qubit_local_files::operation::stream::AsyncLocalDirectoryWalker;
use qubit_local_files::operation::stream::AsyncLocalFileReader;
use qubit_local_files::operation::stream::AsyncLocalFileWriter;
use qubit_local_files::operation::stream::LOCAL_STREAM_BUFFER_SIZE;
use qubit_local_files::operation::stream::LocalStreamCommand;
use qubit_local_files::operation::stream::LocalStreamCompletion;
use qubit_local_files::operation::stream::LocalStreamDriver;
use qubit_local_files::operation::stream::LocalStreamRequest;
use qubit_local_files::operation::stream::LocalStreamResource;
use qubit_local_files::operation::stream::LocalWalkDriver;
use qubit_local_files::operation::stream::LocalWalkResource;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::ResourceRef;
use qubit_operation::ResourceScopeOptions;

#[cfg(feature = "test-support")]
static TEST_STAGE_WRITE_GATE: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct FakeState {
    bytes: Vec<u8>,
    position: usize,
    writes: Vec<u8>,
    write_payloads: Vec<Vec<u8>>,
    hold_next: bool,
    held: Vec<u64>,
    wake: Option<Waker>,
    close_count: usize,
    command_kinds: Vec<&'static str>,
    close_requested: bool,
    partial_write: Option<usize>,
    fail_after_write: Option<usize>,
    write_calls: usize,
    panic_next: bool,
    unconfirmed_next: bool,
    close_error: Option<io::Error>,
    fail_seek: bool,
    oversized_read: Option<Vec<u8>>,
    hold_read_after_execute: bool,
    completed_reads: HashMap<u64, Vec<u8>>,
    commit_calls: usize,
    reported_write_facts: Option<(qubit_local_files::outcome::LocalWriteFailureState, u64)>,
}

#[derive(Default)]
struct FakeWorker(Mutex<FakeState>);

impl FakeWorker {
    fn with_bytes(bytes: &[u8]) -> Self {
        Self(Mutex::new(FakeState {
            bytes: bytes.to_vec(),
            ..FakeState::default()
        }))
    }

    fn hold_next(&self) {
        self.0.lock().unwrap().hold_next = true;
    }

    fn release(&self) {
        let wake = {
            let mut state = self.0.lock().unwrap();
            state.held.clear();
            state.hold_next = false;
            state.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    fn writes(&self) -> Vec<u8> {
        self.0.lock().unwrap().writes.clone()
    }

    fn write_payloads(&self) -> Vec<Vec<u8>> {
        self.0.lock().unwrap().write_payloads.clone()
    }

    fn commit_calls(&self) -> usize {
        self.0.lock().unwrap().commit_calls
    }

    fn command_kinds(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().command_kinds.clone()
    }

    fn partial_then_fail(&self, partial: usize) {
        let mut state = self.0.lock().unwrap();
        state.partial_write = Some(partial);
        state.fail_after_write = Some(1);
    }

    fn panic_next(&self) {
        self.0.lock().unwrap().panic_next = true;
    }

    fn unconfirmed_next(&self) {
        self.0.lock().unwrap().unconfirmed_next = true;
    }

    fn fail_next_close_with_abort_error(&self) {
        self.0.lock().unwrap().close_error = Some(io::Error::new(
            io::ErrorKind::PermissionDenied,
            qubit_local_files::LocalFileError::new(
                qubit_local_files::error::LocalFileErrorKind::InvalidState,
                qubit_local_files::error::LocalFileOperation::Abort,
            ),
        ));
    }

    fn report_write_facts(&self, state: qubit_local_files::outcome::LocalWriteFailureState, bytes: u64) {
        self.0.lock().unwrap().reported_write_facts = Some((state, bytes));
    }

    fn fail_next_seek(&self) {
        self.0.lock().unwrap().fail_seek = true;
    }

    fn overread_next(&self, bytes: Vec<u8>) {
        self.0.lock().unwrap().oversized_read = Some(bytes);
    }

    fn hold_next_read_after_execute(&self) {
        self.0.lock().unwrap().hold_read_after_execute = true;
    }
}

impl LocalStreamDriver for FakeWorker {
    fn poll_command(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
    ) -> Poll<io::Result<LocalStreamCompletion>> {
        let mut state = self.0.lock().unwrap();
        if state.panic_next {
            state.panic_next = false;
            drop(state);
            panic!("fake worker panic");
        }
        if state.unconfirmed_next {
            state.unconfirmed_next = false;
            return Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed {
                operation: "read",
                detail: "fake driver lost after submission".to_owned(),
            }));
        }
        if state.held.contains(&request.id) {
            state.wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        if let Some(bytes) = state.completed_reads.remove(&request.id) {
            return Poll::Ready(Ok(LocalStreamCompletion::Read(bytes)));
        }
        if state.hold_next {
            state.hold_next = false;
            state.held.push(request.id);
            state.wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        state.command_kinds.push(match request.command {
            LocalStreamCommand::Read { .. } => "read",
            LocalStreamCommand::Write { .. } => "write",
            LocalStreamCommand::Flush => "flush",
            LocalStreamCommand::Seek(_) => "seek",
            LocalStreamCommand::Commit => "commit",
            LocalStreamCommand::Close => "close",
            LocalStreamCommand::Abort => "abort",
        });
        let result = match &request.command {
            LocalStreamCommand::Read { max_bytes } => {
                if let Some(bytes) = state.oversized_read.take() {
                    return Poll::Ready(Ok(LocalStreamCompletion::Read(bytes)));
                }
                let end = state.bytes.len().min(state.position.saturating_add(*max_bytes));
                let bytes = state.bytes[state.position..end].to_vec();
                state.position = end;
                if state.hold_read_after_execute {
                    state.hold_read_after_execute = false;
                    state.completed_reads.insert(request.id, bytes);
                    state.held.push(request.id);
                    state.wake = Some(cx.waker().clone());
                    return Poll::Pending;
                }
                Ok(LocalStreamCompletion::Read(bytes))
            }
            LocalStreamCommand::Write { bytes } => {
                state.write_payloads.push(bytes.to_vec());
                if state.fail_after_write == Some(state.write_calls) {
                    return Poll::Ready(Err(io::Error::other("worker lost after partial write")));
                }
                let written = state.partial_write.unwrap_or(bytes.len()).min(bytes.len());
                state.writes.extend_from_slice(&bytes[..written]);
                state.write_calls += 1;
                Ok(LocalStreamCompletion::Written(written))
            }
            LocalStreamCommand::Flush => Ok(LocalStreamCompletion::Flushed),
            LocalStreamCommand::Seek(position) => {
                if state.fail_seek {
                    state.fail_seek = false;
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "fake seek failure",
                    )));
                }
                let next = match position {
                    SeekFrom::Start(offset) => usize::try_from(*offset).unwrap_or(usize::MAX),
                    SeekFrom::Current(offset) => apply_offset(state.position, *offset),
                    SeekFrom::End(offset) => apply_offset(state.bytes.len(), *offset),
                };
                state.position = next;
                Ok(LocalStreamCompletion::Seeked(next as u64))
            }
            LocalStreamCommand::Close => {
                state.close_count += 1;
                if let Some(error) = state.close_error.take() {
                    return Poll::Ready(Err(error));
                }
                Ok(LocalStreamCompletion::Closed)
            }
            LocalStreamCommand::Abort => {
                state.close_count += 1;
                Ok(LocalStreamCompletion::Closed)
            }
            LocalStreamCommand::Commit => {
                state.commit_calls += 1;
                Ok(LocalStreamCompletion::Closed)
            }
        };
        Poll::Ready(result)
    }

    fn request_close(&self, _request_id: u64) {
        self.0.lock().unwrap().close_requested = true;
    }

    fn write_facts(&self) -> Option<(qubit_local_files::outcome::LocalWriteFailureState, u64)> {
        let state = self.0.lock().unwrap();
        Some(state.reported_write_facts.unwrap_or((
            if state.writes.is_empty() {
                qubit_local_files::outcome::LocalWriteFailureState::NotPublished
            } else {
                qubit_local_files::outcome::LocalWriteFailureState::Published
            },
            state.writes.len() as u64,
        )))
    }
}

fn resource(worker: Arc<FakeWorker>) -> ResourceRef<LocalStreamResource> {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let mut operation = manager
        .try_submit(Default::default(), move |scope| {
            let resources = scope.resource_scope(ResourceScopeOptions { capacity: 1 }).unwrap();
            let resource = resources.manage(LocalStreamResource::new(FakeProxy(worker))).unwrap();
            Ok::<_, ()>(resource)
        })
        .unwrap();
    match operation.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("resource registration operation failed"),
    }
}

fn reader(worker: &Arc<FakeWorker>) -> AsyncLocalFileReader {
    AsyncLocalFileReader::new(resource(Arc::clone(worker))).unwrap()
}

fn writer(worker: &Arc<FakeWorker>) -> AsyncLocalFileWriter {
    AsyncLocalFileWriter::new(resource(Arc::clone(worker))).unwrap()
}

#[test]
fn test_async_local_file_streams_report_worker_buffering() {
    let worker = Arc::new(FakeWorker::with_bytes(b"buffered"));
    let reader = reader(&worker);
    let writer = writer(&worker);

    // The reader fills a bounded worker-owned buffer, while the writer stages
    // copied bytes on its serial worker until commit.
    assert!(AsyncInput::is_buffered(&reader));
    assert!(AsyncOutput::is_buffered(&writer));
}

#[test]
fn test_pending_commit_blocks_new_writes_and_preserves_result() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"first")).unwrap(),
        5
    );
    poll_worker(|cx| Box::pin(writer.settle()).as_mut().poll(cx)).unwrap();

    worker.hold_next();
    let mut commit = Box::pin(writer.commit());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(commit.as_mut().poll(&mut cx), Poll::Pending));
    drop(commit);

    let blocked_write = poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"second")).unwrap_err();
    assert_eq!(blocked_write.kind(), io::ErrorKind::InvalidData, "{blocked_write}");
    assert!(blocked_write.to_string().contains("commit is pending"));
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_flush(cx))
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );

    worker.release();
    let first = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert_eq!(worker.commit_calls(), 1);
    assert!(first.to_string().contains("unexpected commit completion"));
}

#[test]
fn test_append_commit_reports_publication_after_later_write_failure() {
    let worker = Arc::new(FakeWorker::default());
    worker.partial_then_fail(2);
    let mut writer = writer(&worker);
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"first batch")).unwrap(),
        11
    );

    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!error.retryable());
    assert_eq!(
        error.commit_error().state(),
        qubit_local_files::outcome::LocalWriteFailureState::Published
    );
    assert_eq!(worker.writes(), b"fi");
    assert!(error.to_string().contains("2 bytes written before failure"));
    let blocked_write =
        match Pin::new(&mut writer).poll_write(&mut Context::from_waker(Waker::noop()), b"must not replay") {
            Poll::Ready(Err(error)) => error,
            other => panic!("terminal append writer must reject a later write, got {other:?}"),
        };
    assert_eq!(blocked_write.kind(), io::ErrorKind::BrokenPipe);
}

#[cfg(feature = "test-support")]
#[test]
fn test_native_append_commit_preserves_bytes_after_later_write_failure() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::LocalPublicationState;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_local_files::outcome::LocalWriteFailureState;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    // The worker consumes a process-wide fault. Isolate it from other stream tests.
    const CHILD: &str = "QUBIT_TEST_NATIVE_APPEND_FAILURE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .env(CHILD, "1")
            .args([
                "--exact",
                "test_native_append_commit_preserves_bytes_after_later_write_failure",
                "--nocapture",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "append test child failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("append.txt");
    std::fs::write(&destination, b"before").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            destination.clone(),
            LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .unwrap();
    let OperationOutcome::Succeeded(mut writer) = opening.wait_outcome().unwrap() else {
        panic!("append writer open failed");
    };

    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b" plus")).unwrap(),
        5
    );
    poll_worker(|cx| Box::pin(writer.settle()).as_mut().poll(cx)).unwrap();
    let fault = install_test_fault_shared("local-writer-write-failed").unwrap();
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b" rejected")).unwrap(),
        9
    );
    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    drop(fault);

    assert_eq!(error.commit_error().state(), LocalWriteFailureState::Published);
    assert!(!error.retryable());
    assert_eq!(observer.snapshot().publication, LocalPublicationState::Published);
    assert_eq!(observer.snapshot().modified_bytes, 5);
    assert_eq!(std::fs::read(destination).unwrap(), b"before plus");
    drop(writer);
    opening.wait_settlement().unwrap();
}

#[test]
fn test_unconfirmed_write_drain_uses_worker_publication_facts() {
    let worker = Arc::new(FakeWorker::default());
    worker.report_write_facts(qubit_local_files::outcome::LocalWriteFailureState::Indeterminate, 4);
    let mut writer = writer(&worker);
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"bytes")).unwrap(),
        5
    );
    worker.unconfirmed_next();

    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!error.retryable());
    assert_eq!(
        error.commit_error().state(),
        qubit_local_files::outcome::LocalWriteFailureState::Indeterminate
    );
    assert!(error.to_string().contains("Unconfirmed"));
    let blocked_write =
        match Pin::new(&mut writer).poll_write(&mut Context::from_waker(Waker::noop()), b"do not replay") {
            Poll::Ready(Err(error)) => error,
            other => panic!("unconfirmed writer must reject a later write, got {other:?}"),
        };
    assert_eq!(blocked_write.kind(), io::ErrorKind::BrokenPipe);
}

#[cfg(feature = "test-support")]
#[test]
fn test_staged_commit_reports_unpublished_failure_and_can_retry() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_local_files::policy::LocalDurabilityRequirement;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("staged-commit.txt");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, _) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            target.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("writer open failed"),
    };
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"staged")).unwrap(),
        6
    );

    let fault = install_test_fault_shared("writer-staged-file-sync").unwrap();
    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(error.retryable());
    assert_eq!(
        error.commit_error().state(),
        qubit_local_files::outcome::LocalWriteFailureState::NotPublished
    );
    assert!(!target.exists());
    drop(fault);

    let outcome = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap();
    assert_eq!(outcome.state(), qubit_local_files::outcome::LocalWriterState::Committed);
    drop(writer);
    opening.wait_settlement().unwrap();
    assert_eq!(std::fs::read(target).unwrap(), b"staged");
}

#[cfg(feature = "test-support")]
#[test]
fn test_closed_writer_retains_commit_error_without_retry_permission() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_local_files::policy::LocalDurabilityRequirement;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_local_files::test_support::install_test_gate;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    let _gate_lock = TEST_STAGE_WRITE_GATE.lock().unwrap();

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("closed-commit-error.txt");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            OperationOptions::default(),
            target.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("staged writer open failed"),
    };

    let gate = install_test_gate("rlf9-writer-after-stage-write").unwrap();
    let mut write = Box::pin(writer.write_fully_async(b"staged"));
    assert!(matches!(poll(|cx| write.as_mut().poll(cx)), Poll::Ready(Ok(()))));
    drop(write);
    let mut settle = Box::pin(writer.settle());
    assert!(poll(|cx| settle.as_mut().poll(cx)).is_pending());
    gate.wait_until_entered();
    drop(settle);

    let mut commit = Box::pin(writer.commit());
    assert!(poll(|cx| commit.as_mut().poll(cx)).is_pending());
    drop(commit);
    let fault = install_test_fault_shared("writer-staged-file-sync").unwrap();
    let mut close = Box::pin(writer.close());
    assert!(poll(|cx| close.as_mut().poll(cx)).is_pending());
    gate.release();
    poll_worker(|cx| close.as_mut().poll(cx)).unwrap();
    drop(close);

    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!error.retryable(), "{error:?}, {}", error.commit_error());
    assert_eq!(
        error.commit_error().state(),
        qubit_local_files::outcome::LocalWriteFailureState::NotPublished
    );
    assert!(error.commit_error().to_string().contains("SyncTemporaryFile"));
    let terminal = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!terminal.retryable());
    assert!(!target.exists());
    drop(fault);
    drop(writer);
    let settlement = opening.wait_settlement().unwrap();
    assert_eq!(settlement.recovery_count, 0);
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::LocalPublicationState::NotPublished
    );
}

#[test]
fn test_close_reports_write_and_cleanup_failures_together() {
    let worker = Arc::new(FakeWorker::default());
    worker.partial_then_fail(2);
    let mut writer = writer(&worker);
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"before close")).unwrap(),
        12
    );
    let write_error = poll_worker(|cx| Box::pin(writer.settle()).as_mut().poll(cx)).unwrap_err();
    assert!(write_error.to_string().contains("2 bytes written before failure"));

    worker.fail_next_close_with_abort_error();
    let error = poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap_err();
    assert!(error.to_string().contains("InvalidState"));
    assert!(error.to_string().contains("2 bytes written before failure"));
    let close_cause = local_file_error_in_chain(&error).expect("close error remains structured");
    assert_eq!(
        close_cause.operation(),
        qubit_local_files::error::LocalFileOperation::Abort
    );
}

#[test]
fn test_close_blocks_new_work_while_draining_accepted_writes() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"accepted")).unwrap(),
        8
    );

    worker.hold_next();
    let mut settle = Box::pin(writer.settle());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(settle.as_mut().poll(&mut cx), Poll::Pending));
    drop(settle);

    let mut close = Box::pin(writer.close());
    assert!(matches!(close.as_mut().poll(&mut cx), Poll::Pending));
    drop(close);

    let write_error = match Pin::new(&mut writer).poll_write(&mut cx, b"late") {
        Poll::Ready(Err(error)) => error,
        other => panic!("write after close starts must fail immediately, got {other:?}"),
    };
    assert_eq!(write_error.kind(), io::ErrorKind::BrokenPipe);
    assert!(write_error.to_string().contains("closing"));
    let flush_error = match Pin::new(&mut writer).poll_flush(&mut cx) {
        Poll::Ready(Err(error)) => error,
        other => panic!("flush after close starts must fail immediately, got {other:?}"),
    };
    assert_eq!(flush_error.kind(), io::ErrorKind::BrokenPipe);
    assert!(flush_error.to_string().contains("closing"));
    let commit_error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(commit_error.to_string().contains("closing"));
    assert!(worker.command_kinds().is_empty(), "accepted commands remain pending");

    worker.release();
    poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap();
    assert_eq!(worker.command_kinds(), vec!["write", "close"]);
    assert_eq!(
        worker.command_kinds().iter().filter(|kind| **kind == "close").count(),
        1
    );
}

#[test]
fn test_close_blocks_new_work_after_close_command_is_queued() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    worker.hold_next();
    let mut close = Box::pin(writer.close());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(close.as_mut().poll(&mut cx), Poll::Pending));
    drop(close);

    let write_error = match Pin::new(&mut writer).poll_write(&mut cx, b"late") {
        Poll::Ready(Err(error)) => error,
        other => panic!("write after close is queued must fail immediately, got {other:?}"),
    };
    assert_eq!(write_error.kind(), io::ErrorKind::BrokenPipe);
    assert!(write_error.to_string().contains("closing"));
    let flush_error = match Pin::new(&mut writer).poll_flush(&mut cx) {
        Poll::Ready(Err(error)) => error,
        other => panic!("flush after close is queued must fail immediately, got {other:?}"),
    };
    assert_eq!(flush_error.kind(), io::ErrorKind::BrokenPipe);
    let commit_error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(commit_error.to_string().contains("closing"));

    worker.release();
    poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap();
    assert_eq!(worker.command_kinds(), vec!["close"]);
}

#[test]
fn test_unpolled_close_future_does_not_block_writer_work() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    let close = Box::pin(writer.close());
    drop(close);

    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"still open")).unwrap(),
        10
    );
    poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap();
    assert_eq!(worker.command_kinds(), vec!["write", "close"]);
}

#[test]
fn test_close_preserves_unobserved_commit_result() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    worker.hold_next();
    let mut commit = Box::pin(writer.commit());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(commit.as_mut().poll(&mut cx), Poll::Pending));
    drop(commit);

    worker.release();
    poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap();
    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(error.to_string().contains("unexpected commit completion"));
    assert_eq!(worker.commit_calls(), 1);
}

#[test]
fn test_close_resumes_commit_cancelled_while_settling_writes() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"pending flush")).unwrap(),
        13
    );

    worker.hold_next();
    let mut commit = Box::pin(writer.commit());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(commit.as_mut().poll(&mut cx), Poll::Pending));
    drop(commit);
    worker.release();

    poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap();
    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!error.retryable());
    assert!(error.to_string().contains("unexpected commit completion"));
    let terminal = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!terminal.retryable());
    assert!(terminal.to_string().contains("already terminal"));
    assert_eq!(worker.commit_calls(), 1);
}

#[cfg(feature = "test-support")]
#[test]
fn test_staged_async_writer_cleanup_failure_can_be_retried() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let _gate_lock = TEST_STAGE_WRITE_GATE.lock().unwrap();

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("staged-abort.txt");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 1,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor.clone()).unwrap();
    let (mut opening, _) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            target.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("writer open failed"),
    };
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"staged")).unwrap(),
        6
    );

    let fault = install_test_fault_shared("writer-staged-abort-cleanup").unwrap();
    let error = poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap_err();
    let cause = local_file_error_in_chain(&error).expect("close error retains LocalFileError");
    assert_eq!(cause.operation(), qubit_local_files::error::LocalFileOperation::Abort);
    let repeated = poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap_err();
    let repeated_cause = local_file_error_in_chain(&repeated).expect("repeated close retains LocalFileError");
    assert_eq!(
        repeated_cause.operation(),
        qubit_local_files::error::LocalFileOperation::Abort
    );
    assert!(error.to_string().contains("Abort"));
    assert!(
        !target.exists(),
        "failed staged cleanup must not publish the destination"
    );

    drop(writer);
    let settlement = opening.wait_settlement().unwrap();
    assert_eq!(settlement.recovery_count, 1);
    let recovery = settlement
        .recoveries
        .first()
        .expect("settlement retains failed staged cleanup")
        .clone();
    let probe = directory.path().join("handle-probe.txt");
    let rejected = match filesystem.submit_open_writer(
        &manager,
        Default::default(),
        probe.clone(),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    ) {
        Err(error) => error,
        Ok(_) => panic!("failed native cleanup must retain its handle reservation"),
    };
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    drop(fault);
    recovery.try_retry().unwrap().wait().unwrap();
    let (mut probe_open, _) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            probe,
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("successful native cleanup recovery must return its handle reservation");
    let mut probe_writer = match probe_open.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("probe writer failed to open after cleanup recovery"),
    };
    poll_worker(|cx| Box::pin(probe_writer.close()).as_mut().poll(cx)).unwrap();
    drop(probe_writer);
    probe_open.wait_settlement().unwrap();
    assert!(!target.exists());
    assert_eq!(
        std::fs::read_dir(directory.path()).unwrap().count(),
        0,
        "the retained worker owner must retry and remove its staging entry"
    );

    let rooted_directory = tempfile::tempdir().unwrap();
    let rooted_manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let rooted_executor = LocalIoExecutor::new(rooted_manager.clone(), LocalExecutionConfig::default()).unwrap();
    let rooted_filesystem = LocalFileSystem::rooted(rooted_directory.path())
        .unwrap()
        .into_async(rooted_executor)
        .unwrap();
    let rooted_target = std::path::PathBuf::from("staged-abort.txt");
    let (mut rooted_opening, _) = rooted_filesystem
        .submit_open_writer(
            &rooted_manager,
            Default::default(),
            rooted_target,
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut rooted_writer = match rooted_opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("Rooted writer open failed"),
    };
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut rooted_writer).poll_write(cx, b"staged")).unwrap(),
        6
    );
    let fault = install_test_fault_shared("writer-staged-abort-cleanup").unwrap();
    let error = poll_worker(|cx| Box::pin(rooted_writer.close()).as_mut().poll(cx)).unwrap_err();
    drop(fault);
    assert!(error.to_string().contains("Abort"));
    rooted_opening.wait_settlement().unwrap();
    assert_eq!(
        std::fs::read_dir(rooted_directory.path()).unwrap().count(),
        0,
        "Rooted recovery must retain ownership and remove the staging entry"
    );
}

#[derive(Default)]
struct FakeWalkState {
    batches: VecDeque<Option<Vec<PathBuf>>>,
    calls: usize,
    request_ids: Vec<u64>,
    hold_next_batch: bool,
    held_request_id: Option<u64>,
    batch_released: bool,
    stopped: bool,
    closed: bool,
    allow_close: bool,
    wake: Option<Waker>,
}

#[derive(Default)]
struct FakeWalkWorker(Mutex<FakeWalkState>);

impl FakeWalkWorker {
    fn with_batches(batches: impl IntoIterator<Item = Option<Vec<PathBuf>>>) -> Self {
        Self(Mutex::new(FakeWalkState {
            batches: batches.into_iter().collect(),
            allow_close: true,
            ..FakeWalkState::default()
        }))
    }

    fn release_close(&self) {
        let wake = {
            let mut state = self.0.lock().unwrap();
            state.allow_close = true;
            state.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    fn hold_next_batch(&self) {
        self.0.lock().unwrap().hold_next_batch = true;
    }

    fn release_batch(&self) {
        let wake = {
            let mut state = self.0.lock().unwrap();
            state.batch_released = true;
            state.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

impl LocalWalkDriver for FakeWalkWorker {
    fn poll_batch(
        &self,
        cx: &mut Context<'_>,
        request_id: u64,
        _max_entries: usize,
        _max_bytes: usize,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        let mut state = self.0.lock().unwrap();
        state.calls += 1;
        state.request_ids.push(request_id);
        if state.hold_next_batch {
            state.hold_next_batch = false;
            state.held_request_id = Some(request_id);
        }
        if state.held_request_id == Some(request_id) && !state.batch_released {
            state.wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        if state.held_request_id == Some(request_id) {
            state.held_request_id = None;
            state.batch_released = false;
        }
        if state.stopped {
            return Poll::Ready(Ok(None));
        }
        Poll::Ready(Ok(state.batches.pop_front().unwrap_or(None)))
    }

    fn request_stop(&self) {
        self.0.lock().unwrap().stopped = true;
    }

    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self.0.lock().unwrap();
        if state.allow_close {
            state.closed = true;
            Poll::Ready(Ok(()))
        } else {
            state.wake = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[test]
fn test_walker_cancelled_batch_future_reuses_request_id() {
    let worker = Arc::new(FakeWalkWorker::with_batches([Some(vec![PathBuf::from("entry")])]));
    worker.hold_next_batch();
    let mut walker = AsyncLocalDirectoryWalker::new(walk_resource(Arc::clone(&worker))).unwrap();
    let mut first = Box::pin(walker.next_batch());
    assert!(matches!(poll(|cx| first.as_mut().poll(cx)), Poll::Pending));
    drop(first);
    worker.release_batch();
    assert!(matches!(
        poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)),
        Poll::Ready(Ok(Some(entries))) if entries == [PathBuf::from("entry")]
    ));
    let state = worker.0.lock().unwrap();
    assert_eq!(state.request_ids.len(), 2);
    assert_eq!(state.request_ids[0], state.request_ids[1]);
}

fn walk_resource(worker: Arc<FakeWalkWorker>) -> ResourceRef<LocalWalkResource> {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let mut operation = manager
        .try_submit(Default::default(), move |scope| {
            let resources = scope.resource_scope(ResourceScopeOptions { capacity: 1 }).unwrap();
            let resource = resources.manage(LocalWalkResource::new(FakeWalkProxy(worker))).unwrap();
            Ok::<_, ()>(resource)
        })
        .unwrap();
    match operation.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("walker resource registration failed"),
    }
}

#[derive(Clone)]
struct FakeWalkProxy(Arc<FakeWalkWorker>);

impl LocalWalkDriver for FakeWalkProxy {
    fn poll_batch(
        &self,
        cx: &mut Context<'_>,
        request_id: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        self.0.poll_batch(cx, request_id, max_entries, max_bytes)
    }

    fn request_stop(&self) {
        self.0.request_stop();
    }

    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.poll_closed(cx)
    }
}

fn apply_offset(base: usize, offset: i64) -> usize {
    if offset >= 0 {
        base.saturating_add(offset as usize)
    } else {
        base.saturating_sub(offset.unsigned_abs() as usize)
    }
}

// ResourceRef requires an owned Send + Sync resource; this proxy gives each
// handle a distinct registration while the fake worker remains under test
// control.
#[derive(Clone)]
struct FakeProxy(Arc<FakeWorker>);

impl LocalStreamDriver for FakeProxy {
    fn poll_command(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
    ) -> Poll<io::Result<LocalStreamCompletion>> {
        self.0.poll_command(cx, request)
    }

    fn request_close(&self, request_id: u64) {
        self.0.request_close(request_id);
    }

    fn write_facts(&self) -> Option<(qubit_local_files::outcome::LocalWriteFailureState, u64)> {
        self.0.write_facts()
    }
}

fn poll<T>(future: impl FnOnce(&mut Context<'_>) -> Poll<T>) -> Poll<T> {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    future(&mut cx)
}

fn poll_worker<T>(mut future: impl FnMut(&mut Context<'_>) -> Poll<T>) -> T {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(value) = future(&mut cx) {
            return value;
        }
        assert!(Instant::now() < deadline, "worker-backed operation did not complete");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn local_file_error_in_chain<'a>(error: &'a io::Error) -> Option<&'a qubit_local_files::LocalFileError> {
    let mut pending: Vec<&'a (dyn std::error::Error + 'static)> = Vec::new();
    pending.extend(
        error
            .get_ref()
            .map(|source| source as &(dyn std::error::Error + 'static)),
    );
    while let Some(source) = pending.pop() {
        if let Some(file_error) = source.downcast_ref::<qubit_local_files::LocalFileError>() {
            return Some(file_error);
        }
        if let Some(io_error) = source.downcast_ref::<io::Error>() {
            pending.extend(
                io_error
                    .get_ref()
                    .map(|source| source as &(dyn std::error::Error + 'static)),
            );
        }
        pending.extend(std::error::Error::source(source));
    }
    None
}

#[test]
fn test_async_filesystem_opens_real_reader_writer_and_bounded_walker() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::options::LocalReadOptions;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.txt");
    let output = directory.path().join("output.txt");
    std::fs::write(&input, b"real local reader").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();

    let mut opening = filesystem
        .submit_open_reader(&manager, Default::default(), input.clone(), LocalReadOptions::default())
        .unwrap();
    let mut reader = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(reader) => reader,
        _ => panic!("reader open failed"),
    };
    let mut bytes = [0; 64];
    let count = poll_worker(|cx| Pin::new(&mut reader).poll_read(cx, &mut bytes)).unwrap();
    assert_eq!(&bytes[..count], b"real local reader");
    let mut seek = Box::pin(reader.seek(SeekFrom::End(-6)));
    assert_eq!(poll_worker(|cx| seek.as_mut().poll(cx)).unwrap(), 11);
    drop(seek);
    let count = poll_worker(|cx| Pin::new(&mut reader).poll_read(cx, &mut bytes)).unwrap();
    assert_eq!(&bytes[..count], b"reader");
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut reader).poll_read(cx, &mut bytes)).unwrap(),
        0
    );
    poll_worker(|cx| Box::pin(reader.close()).as_mut().poll(cx)).unwrap();
    drop(reader);
    opening.wait_settlement().unwrap();

    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            output.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("writer open failed"),
    };
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"written on worker")).unwrap(),
        17
    );
    poll_worker(|cx| Pin::new(&mut writer).poll_flush(cx)).unwrap();
    assert!(!output.exists(), "flush must not publish an uncommitted writer");
    let outcome = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap();
    assert_eq!(outcome.state(), qubit_local_files::outcome::LocalWriterState::Committed);
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::LocalPublicationState::Published
    );
    let error = match Pin::new(&mut writer).poll_write(&mut Context::from_waker(Waker::noop()), b"late") {
        Poll::Ready(Err(error)) => error,
        other => panic!("terminal writer must reject later writes, got {other:?}"),
    };
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!error.retryable());
    assert!(error.to_string().contains("already terminal"));
    drop(writer);
    opening.wait_settlement().unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), b"written on worker");

    let aborted = directory.path().join("aborted.txt");
    let (mut opening, _) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            aborted.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("writer open failed"),
    };
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"must abort")).unwrap(),
        10
    );
    drop(writer);
    opening.wait_settlement().unwrap();
    assert!(!aborted.exists(), "dropping an uncommitted writer must abort");

    let mut opening = filesystem
        .submit_open_walker(
            &manager,
            Default::default(),
            directory.path().to_path_buf(),
            LocalListOptions::default(),
        )
        .unwrap();
    let mut walker = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(walker) => walker,
        _ => panic!("walker open failed"),
    };
    let mut found = Vec::new();
    while let Some(batch) = poll_worker(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)).unwrap() {
        found.extend(batch);
    }
    assert!(
        found
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "input.txt"))
    );
    assert!(
        found
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "output.txt"))
    );
    drop(walker);
    opening.wait_settlement().unwrap();
}

#[test]
fn test_real_worker_stream_preserves_large_write_order_through_flush_and_commit() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("large-stream.bin");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, _) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            output.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("writer open failed"),
    };

    let expected = (0..(LOCAL_STREAM_BUFFER_SIZE * 2 + 17))
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let mut offset = 0;
    while offset < expected.len() {
        let written = poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, &expected[offset..])).unwrap();
        assert!(written > 0, "a non-empty worker write must make progress");
        offset += written;
    }
    poll_worker(|cx| Pin::new(&mut writer).poll_flush(cx)).unwrap();
    assert!(!output.exists(), "flush must not publish an uncommitted writer");
    let outcome = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap();
    assert_eq!(outcome.state(), qubit_local_files::outcome::LocalWriterState::Committed);
    drop(writer);
    opening.wait_settlement().unwrap();

    assert_eq!(std::fs::read(output).unwrap(), expected);
}

#[test]
fn test_terminal_real_worker_writer_releases_single_handle_for_reopen() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    #[cfg(feature = "test-support")]
    let _gate_lock = TEST_STAGE_WRITE_GATE.lock().unwrap();

    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("single-handle.txt");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 1,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();

    let (mut abandoned_open, _) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            output.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut abandoned = match abandoned_open.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("initial worker writer failed to open"),
    };
    poll_worker(|cx| Box::pin(abandoned.close()).as_mut().poll(cx)).unwrap();
    drop(abandoned);
    abandoned_open.wait_settlement().unwrap();
    assert!(
        !output.exists(),
        "dropping an uncommitted writer must abort its publication"
    );

    let (mut replacement_open, _) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            output.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("settled worker writer must release its handle for reopen");
    let mut replacement = match replacement_open.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("worker writer did not reopen after abort cleanup"),
    };
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut replacement).poll_write(cx, b"published")).unwrap(),
        9
    );
    let outcome = poll_worker(|cx| Box::pin(replacement.commit()).as_mut().poll(cx)).unwrap();
    assert_eq!(outcome.state(), qubit_local_files::outcome::LocalWriterState::Committed);
    drop(replacement);
    replacement_open.wait_settlement().unwrap();
    assert_eq!(std::fs::read(output).unwrap(), b"published");
}

#[test]
fn test_real_writer_commit_preserves_typed_error_and_observer() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("race.txt");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            destination.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("writer open failed"),
    };
    poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b"staged")).unwrap();
    std::fs::write(&destination, b"racer").unwrap();
    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap_err();
    assert!(!error.retryable(), "{error:?}, {}", error.commit_error());
    assert_eq!(
        error.commit_error().state(),
        qubit_local_files::outcome::LocalWriteFailureState::NotPublished,
        "{error}"
    );
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::LocalPublicationState::NotPublished
    );
    drop(writer);
    opening.wait_settlement().unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"racer");
}

#[test]
fn test_append_facts_survive_close_and_drop() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();

    let closed_path = directory.path().join("closed.txt");
    std::fs::write(&closed_path, b"before").unwrap();
    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            closed_path.clone(),
            LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("append writer open failed"),
    };
    poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b" plus close")).unwrap();
    poll_worker(|cx| Box::pin(writer.settle()).as_mut().poll(cx)).unwrap();
    poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap();
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::LocalPublicationState::Published
    );
    assert_eq!(observer.snapshot().modified_bytes, 11);
    drop(writer);
    opening.wait_settlement().unwrap();
    assert_eq!(std::fs::read(&closed_path).unwrap(), b"before plus close");

    let dropped_path = directory.path().join("dropped.txt");
    std::fs::write(&dropped_path, b"before").unwrap();
    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            dropped_path.clone(),
            LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("append writer open failed"),
    };
    poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b" plus drop")).unwrap();
    poll_worker(|cx| Box::pin(writer.settle()).as_mut().poll(cx)).unwrap();
    drop(writer);
    opening.wait_settlement().unwrap();
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::LocalPublicationState::Published
    );
    assert_eq!(observer.snapshot().modified_bytes, 10);
    assert_eq!(std::fs::read(&dropped_path).unwrap(), b"before plus drop");
}

#[test]
fn test_empty_append_commit_does_not_record_publication() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("empty-append.txt");
    std::fs::write(&destination, b"existing").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            destination.clone(),
            LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("append writer open failed"),
    };
    let outcome = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap();
    assert_eq!(outcome.state(), qubit_local_files::outcome::LocalWriterState::Committed);
    assert_eq!(std::fs::read(&destination).unwrap(), b"existing");
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::LocalPublicationState::NotPublished
    );
    assert_eq!(observer.snapshot().modified_count, 0);
    assert_eq!(observer.snapshot().modified_bytes, 0);
    drop(writer);
    opening.wait_settlement().unwrap();
}

#[cfg(feature = "test-support")]
#[test]
fn test_real_writer_close_finishes_commit_started_before_enqueue() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_local_files::test_support::install_test_gate;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    let _gate_lock = TEST_STAGE_WRITE_GATE.lock().unwrap();

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("close-resumes-commit.txt");
    std::fs::write(&destination, b"old").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, _observer) = filesystem
        .submit_open_writer(
            &manager,
            OperationOptions::default(),
            destination.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("staged writer open failed"),
    };

    let gate = install_test_gate("rlf9-writer-after-stage-write").unwrap();
    let mut write = Box::pin(writer.write_fully_async(b"new contents"));
    assert!(matches!(poll(|cx| write.as_mut().poll(cx)), Poll::Ready(Ok(()))));
    drop(write);
    let mut settle = Box::pin(writer.settle());
    assert!(poll(|cx| settle.as_mut().poll(cx)).is_pending());
    gate.wait_until_entered();
    drop(settle);
    assert_eq!(std::fs::read(&destination).unwrap(), b"old");

    let mut commit = Box::pin(writer.commit());
    assert!(poll(|cx| commit.as_mut().poll(cx)).is_pending());
    drop(commit);
    assert_eq!(std::fs::read(&destination).unwrap(), b"old");
    let blocked_write = poll_worker(|cx| Box::pin(writer.write_fully_async(b"later")).as_mut().poll(cx)).unwrap_err();
    assert_eq!(blocked_write.kind(), io::ErrorKind::InvalidData, "{blocked_write}");
    assert!(blocked_write.to_string().contains("commit is pending"));

    let mut close = Box::pin(writer.close());
    assert!(poll(|cx| close.as_mut().poll(cx)).is_pending());
    gate.release();
    poll_worker(|cx| close.as_mut().poll(cx)).unwrap();
    drop(close);

    let outcome = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap();
    assert_eq!(outcome.state(), qubit_local_files::outcome::LocalWriterState::Committed);
    assert_eq!(std::fs::read(&destination).unwrap(), b"new contents");
    drop(writer);
    let settlement = opening.wait_settlement().unwrap();
    assert_eq!(settlement.recovery_count, 0);
}

#[test]
fn test_empty_staged_commit_records_publication() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("empty-staged.txt");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            destination.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("staged writer open failed"),
    };
    let outcome = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx)).unwrap();
    assert_eq!(outcome.state(), qubit_local_files::outcome::LocalWriterState::Committed);
    assert_eq!(std::fs::read(&destination).unwrap(), b"");
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::LocalPublicationState::Published
    );
    assert_eq!(observer.snapshot().modified_count, 1);
    assert_eq!(observer.snapshot().modified_bytes, 0);
    drop(writer);
    opening.wait_settlement().unwrap();
}

#[test]
fn test_rooted_worker_streams_keep_the_captured_authority_after_path_replacement() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("payload"), b"captured stream").unwrap();
    std::fs::write(root.join("append"), b"before").unwrap();

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::rooted(&root).unwrap().into_async(executor).unwrap();
    std::fs::rename(&root, scratch.path().join("moved-root")).unwrap();
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("payload"), b"replacement path").unwrap();
    std::fs::write(root.join("append"), b"replacement").unwrap();

    let mut opening = filesystem
        .submit_open_reader(
            &manager,
            Default::default(),
            PathBuf::from("payload"),
            qubit_local_files::options::LocalReadOptions::default(),
        )
        .unwrap();
    let mut reader = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(reader) => reader,
        _ => panic!("rooted reader failed to open"),
    };
    let mut contents = [0; 32];
    let count = poll_worker(|cx| Pin::new(&mut reader).poll_read(cx, &mut contents)).unwrap();
    assert_eq!(&contents[..count], b"captured stream");
    poll_worker(|cx| Box::pin(reader.close()).as_mut().poll(cx)).unwrap();
    drop(reader);
    opening.wait_settlement().unwrap();

    let (mut opening, observer) = filesystem
        .submit_open_writer(
            &manager,
            Default::default(),
            PathBuf::from("append"),
            LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .unwrap();
    let mut writer = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(writer) => writer,
        _ => panic!("rooted append writer failed to open"),
    };
    poll_worker(|cx| Pin::new(&mut writer).poll_write(cx, b" plus stream")).unwrap();
    poll_worker(|cx| Box::pin(writer.close()).as_mut().poll(cx)).unwrap();
    assert_eq!(observer.snapshot().modified_bytes, 12);
    drop(writer);
    opening.wait_settlement().unwrap();
    assert_eq!(
        std::fs::read(scratch.path().join("moved-root/append")).unwrap(),
        b"before plus stream"
    );
    assert_eq!(std::fs::read(root.join("append")).unwrap(), b"replacement");
}

#[test]
fn test_walker_batches_return_every_entry_on_portable_paths() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalListOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let mut root = directory.path().to_path_buf();
    for index in 0..13 {
        // Keep the full path below macOS's PATH_MAX while retaining a deeply
        // nested fixture on every platform.
        root.push(format!("d{index}_{}", "x".repeat(60)));
        std::fs::create_dir(&root).unwrap();
    }
    for index in 0..128 {
        std::fs::write(root.join(format!("item-{index:03}.txt")), b"").unwrap();
    }
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let mut opening = filesystem
        .submit_open_walker(
            &manager,
            Default::default(),
            directory.path().to_path_buf(),
            LocalListOptions::default().with_recursive(),
        )
        .unwrap();
    let mut walker = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(walker) => walker,
        _ => panic!("walker open failed"),
    };
    let mut found = Vec::new();
    let mut batch_count = 0;
    while let Some(batch) = poll_worker(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)).unwrap() {
        assert!(!batch.is_empty(), "a live iterator must not yield empty batches");
        assert!(batch.len() <= 128, "worker batch must respect the entry limit");
        batch_count += 1;
        found.extend(batch);
    }
    assert!(batch_count > 1, "the real iterator must continue across batches");
    assert_eq!(
        found
            .iter()
            .filter(|path| path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("item-")))
            .count(),
        128
    );
    drop(walker);
    opening.wait_settlement().unwrap();
}

#[test]
fn test_stream_handle_permit_lives_until_worker_cleanup_settles() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalReadOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.txt");
    std::fs::write(&path, b"x").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let mut opening = filesystem
        .submit_open_reader(&manager, Default::default(), path.clone(), LocalReadOptions::default())
        .unwrap();
    let mut reader = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(reader) => reader,
        _ => panic!("reader open failed"),
    };
    let rejected =
        match filesystem.submit_open_reader(&manager, Default::default(), path.clone(), LocalReadOptions::default()) {
            Ok(_) => panic!("second stream should exceed handle capacity"),
            Err(error) => error,
        };
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    poll_worker(|cx| Box::pin(reader.close()).as_mut().poll(cx)).unwrap();
    drop(reader);
    opening.wait_settlement().unwrap();
    let mut second = filesystem
        .submit_open_reader(&manager, Default::default(), path, LocalReadOptions::default())
        .unwrap();
    let mut reader = match second.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(reader) => reader,
        _ => panic!("second reader open failed"),
    };
    poll_worker(|cx| Box::pin(reader.close()).as_mut().poll(cx)).unwrap();
    drop(reader);
    second.wait_settlement().unwrap();
}

#[test]
fn test_pending_read_future_drop_keeps_the_first_chunk_for_the_next_read() {
    let worker = Arc::new(FakeWorker::with_bytes(b"first chunk"));
    worker.hold_next();
    let mut reader = reader(&worker);
    let mut abandoned = [0; 5];
    assert!(poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut abandoned)).is_pending());

    worker.release();
    let mut output = [0; 16];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)),
        Poll::Ready(Ok(11))
    ));
    assert_eq!(&output[..11], b"first chunk");
}

#[test]
fn test_settle_rejects_and_latches_over_limit_read_completion() {
    let worker = Arc::new(FakeWorker::default());
    worker.hold_next();
    worker.overread_next(vec![b'x'; LOCAL_STREAM_BUFFER_SIZE + 1]);
    let mut reader = reader(&worker);
    let mut output = [0; 1];
    assert!(poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)).is_pending());

    worker.release();
    let error = match poll(|cx| Box::pin(reader.settle()).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected over-limit read to fail during settle, got {other:?}"),
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)),
        Poll::Ready(Err(error)) if error.kind() == io::ErrorKind::InvalidData
    ));
}

#[test]
fn test_read_rejects_completion_with_over_limit_capacity() {
    let worker = Arc::new(FakeWorker::default());
    let mut allocation = Vec::with_capacity(LOCAL_STREAM_BUFFER_SIZE + 1);
    allocation.extend_from_slice(b"valid length");
    worker.overread_next(allocation);
    let mut reader = reader(&worker);
    let mut output = [0; 1];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)),
        Poll::Ready(Err(error)) if error.kind() == io::ErrorKind::InvalidData
    ));
}

#[test]
fn test_close_rejects_over_limit_read_completion_before_release() {
    let worker = Arc::new(FakeWorker::default());
    worker.hold_next();
    worker.overread_next(vec![b'x'; LOCAL_STREAM_BUFFER_SIZE + 1]);
    let mut reader = reader(&worker);
    let mut output = [0; 1];
    assert!(poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)).is_pending());

    let mut close = Box::pin(reader.close());
    assert!(poll(|cx| close.as_mut().poll(cx)).is_pending());
    worker.release();
    let error = match poll(|cx| close.as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected over-limit read to fail during close, got {other:?}"),
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(worker.0.lock().unwrap().close_count, 1);
}

#[test]
fn test_writes_are_accepted_only_when_owned_capacity_is_available() {
    let worker = Arc::new(FakeWorker::default());
    worker.hold_next();
    let mut writer = writer(&worker);
    let large = vec![b'a'; 64 * 1024];
    assert!(
        matches!(poll(|cx| Pin::new(&mut writer).poll_write(cx, &large)), Poll::Ready(Ok(large_len)) if large_len == large.len())
    );
    let abandoned_input = b"old buffer".to_vec();
    assert!(poll(|cx| Pin::new(&mut writer).poll_write(cx, &abandoned_input)).is_pending());
    assert!(worker.writes().is_empty(), "pending write must not reach worker");

    worker.release();
    drop(abandoned_input);
    let replacement = b"new bytes";
    assert!(matches!(
        poll(|cx| Pin::new(&mut writer).poll_write(cx, replacement)),
        Poll::Ready(Ok(9))
    ));
    assert!(matches!(
        poll(|cx| Pin::new(&mut writer).poll_flush(cx)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(worker.writes(), [large, replacement.to_vec()].concat());
}

#[test]
fn test_flush_waits_until_already_accepted_write_finishes() {
    let worker = Arc::new(FakeWorker::default());
    worker.hold_next();
    let mut writer = writer(&worker);
    assert!(matches!(
        poll(|cx| Pin::new(&mut writer).poll_write(cx, b"accepted")),
        Poll::Ready(Ok(8))
    ));
    assert!(poll(|cx| Pin::new(&mut writer).poll_flush(cx)).is_pending());
    assert!(worker.writes().is_empty());
    worker.release();
    assert!(matches!(
        poll(|cx| Pin::new(&mut writer).poll_flush(cx)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(worker.writes(), b"accepted");
}

#[test]
fn test_ready_write_accepts_bytes_in_the_same_poll_and_zero_count_is_immediate() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    assert!(matches!(
        poll(|cx| unsafe { Pin::new(&mut writer).poll_write_unchecked(cx, b"range", 1, 3) }),
        Poll::Ready(Ok(3))
    ));
    assert!(matches!(
        poll(|cx| Pin::new(&mut writer).poll_write(cx, b"")),
        Poll::Ready(Ok(0))
    ));
    assert!(matches!(
        poll(|cx| Pin::new(&mut writer).poll_flush(cx)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(worker.writes(), b"ang");
}

#[test]
fn test_seek_current_accounts_for_prefetched_bytes() {
    let worker = Arc::new(FakeWorker::with_bytes(b"0123456789"));
    let mut reader = reader(&worker);
    let mut first = [0; 2];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut first)),
        Poll::Ready(Ok(2))
    ));
    assert_eq!(&first, b"01");
    let mut seek = Box::pin(reader.seek(SeekFrom::Current(1)));
    assert!(matches!(poll(|cx| seek.as_mut().poll(cx)), Poll::Ready(Ok(3))));
    drop(seek);
    let mut next = [0; 1];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut next)),
        Poll::Ready(Ok(1))
    ));
    assert_eq!(&next, b"3");
}

#[test]
fn test_pending_seek_future_repoll_returns_original_position_without_reissuing_seek() {
    let worker = Arc::new(FakeWorker::with_bytes(b"abc"));
    worker.hold_next();
    let mut reader = reader(&worker);
    let mut seek = Box::pin(reader.seek(SeekFrom::Current(1)));
    assert!(poll(|cx| seek.as_mut().poll(cx)).is_pending());

    worker.release();
    assert!(matches!(poll(|cx| seek.as_mut().poll(cx)), Poll::Ready(Ok(1))));
    drop(seek);

    let mut output = [0; 1];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)),
        Poll::Ready(Ok(1))
    ));
    assert_eq!(output, [b'b']);
}

#[test]
fn test_new_seek_future_waits_for_cancelled_seek_then_runs_its_own_request() {
    let worker = Arc::new(FakeWorker::with_bytes(b"abc"));
    worker.hold_next();
    let mut reader = reader(&worker);

    let mut cancelled_seek = Box::pin(reader.seek(SeekFrom::Current(0)));
    assert!(poll(|cx| cancelled_seek.as_mut().poll(cx)).is_pending());
    drop(cancelled_seek);

    let mut next_seek = Box::pin(reader.seek(SeekFrom::Current(1)));
    assert!(poll(|cx| next_seek.as_mut().poll(cx)).is_pending());
    worker.release();
    assert!(matches!(poll(|cx| next_seek.as_mut().poll(cx)), Poll::Ready(Ok(1))));
    drop(next_seek);

    let mut output = [0; 1];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)),
        Poll::Ready(Ok(1))
    ));
    assert_eq!(output, [b'b']);
}

#[test]
fn test_cancelled_seek_remains_a_barrier_until_its_position_is_decided() {
    let worker = Arc::new(FakeWorker::with_bytes(b"0123456789"));
    let mut reader = reader(&worker);
    let mut first = [0; 1];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut first)),
        Poll::Ready(Ok(1))
    ));
    worker.hold_next();
    let mut seek = Box::pin(reader.seek(SeekFrom::Current(0)));
    assert!(poll(|cx| seek.as_mut().poll(cx)).is_pending());
    drop(seek);

    let mut next = [0; 1];
    assert!(poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut next)).is_pending());
    worker.release();
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut next)),
        Poll::Ready(Ok(1))
    ));
    assert_eq!(&next, b"1");
}

#[test]
fn test_current_seek_settles_completed_but_unconsumed_read_before_adjusting_offset() {
    let mut bytes = vec![b'a'; 128 * 1024];
    bytes[1] = b'b';
    let worker = Arc::new(FakeWorker::with_bytes(&bytes));
    worker.hold_next_read_after_execute();
    let mut reader = reader(&worker);
    let mut output = [0; 1];
    assert!(poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)).is_pending());

    let mut seek = Box::pin(reader.seek(SeekFrom::Current(1)));
    assert!(poll(|cx| seek.as_mut().poll(cx)).is_pending());
    worker.release();
    assert!(matches!(poll(|cx| seek.as_mut().poll(cx)), Poll::Ready(Ok(1))));
    drop(seek);

    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut output)),
        Poll::Ready(Ok(1))
    ));
    assert_eq!(output, [b'b']);
}

#[test]
fn test_failed_seek_marks_position_unknown_and_never_delivers_old_prefetch() {
    let worker = Arc::new(FakeWorker::with_bytes(b"0123456789"));
    let mut reader = reader(&worker);
    let mut first = [0; 1];
    assert!(matches!(
        poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut first)),
        Poll::Ready(Ok(1))
    ));
    worker.fail_next_seek();
    let mut seek = Box::pin(reader.seek(SeekFrom::Current(0)));
    assert!(
        matches!(poll(|cx| seek.as_mut().poll(cx)), Poll::Ready(Err(error)) if error.to_string().contains("position unknown"))
    );
    drop(seek);

    let mut next = [0; 1];
    assert!(
        matches!(poll(|cx| Pin::new(&mut reader).poll_read(cx, &mut next)), Poll::Ready(Err(error)) if error.to_string().contains("position unknown"))
    );
}

#[test]
fn test_close_is_independent_of_scope_settlement_and_is_repeatable() {
    let worker = Arc::new(FakeWorker::default());
    let mut writer = writer(&worker);
    assert!(matches!(
        poll(|cx| Box::pin(writer.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(worker.0.lock().unwrap().close_count, 1);
    assert!(matches!(
        poll(|cx| Box::pin(writer.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(worker.0.lock().unwrap().close_count, 1);
}

#[test]
fn test_close_completes_while_operation_scope_is_still_open() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let worker = Arc::new(FakeWorker::default());
    let worker_for_operation = Arc::clone(&worker);
    let (resource_tx, resource_rx) = sync_channel(1);
    let (continue_tx, continue_rx) = sync_channel(0);
    let mut operation = manager
        .try_submit(Default::default(), move |scope| {
            let resources = scope.resource_scope(ResourceScopeOptions { capacity: 1 }).unwrap();
            let resource = resources
                .manage(LocalStreamResource::new(FakeProxy(worker_for_operation)))
                .unwrap();
            resource_tx.send(resource).unwrap();
            continue_rx.recv().unwrap();
            Ok::<_, ()>(())
        })
        .unwrap();
    let resource = resource_rx.recv().unwrap();
    let mut writer = AsyncLocalFileWriter::new(resource).unwrap();
    assert!(matches!(
        poll(|cx| Box::pin(writer.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(worker.0.lock().unwrap().close_count, 1);
    continue_tx.send(()).unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));
    operation.wait_settlement().unwrap();
}

#[test]
fn test_stream_drop_requests_worker_close_without_blocking() {
    let worker = Arc::new(FakeWorker::default());
    let writer = writer(&worker);
    drop(writer);
    assert!(worker.0.lock().unwrap().close_requested);
    assert_eq!(worker.0.lock().unwrap().close_count, 0, "drop only enqueues close");
}

#[test]
fn test_cancelled_close_can_be_observed_by_a_later_close_call() {
    let worker = Arc::new(FakeWorker::default());
    worker.hold_next();
    let mut writer = writer(&worker);
    let mut close = Box::pin(writer.close());
    assert!(poll(|cx| close.as_mut().poll(cx)).is_pending());
    drop(close);
    worker.release();
    assert!(matches!(
        poll(|cx| Box::pin(writer.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(worker.0.lock().unwrap().close_count, 1);
}

#[test]
fn test_cloned_resource_reference_cannot_create_a_second_mutable_stream() {
    let worker = Arc::new(FakeWorker::default());
    let resource = resource(Arc::clone(&worker));
    let second_reference = resource.clone();
    let first = AsyncLocalFileReader::new(resource).unwrap();
    assert!(AsyncLocalFileWriter::new(second_reference).is_err());
    drop(first);
}

#[test]
fn test_partial_write_failure_latches_actual_count_and_driver_panic_is_unconfirmed() {
    let worker = Arc::new(FakeWorker::default());
    worker.partial_then_fail(2);
    worker.report_write_facts(qubit_local_files::outcome::LocalWriteFailureState::NotPublished, 2);
    let mut writer = writer(&worker);
    assert!(matches!(
        poll(|cx| Pin::new(&mut writer).poll_write(cx, b"abcdef")),
        Poll::Ready(Ok(6))
    ));
    let error = match poll(|cx| Pin::new(&mut writer).poll_flush(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected latched worker failure, got {other:?}"),
    };
    assert!(error.to_string().contains("2 bytes written before failure"));
    assert_eq!(worker.writes(), b"ab");
    assert_eq!(worker.write_payloads(), [b"abcdef".to_vec(), b"cdef".to_vec()]);

    let error = poll_worker(|cx| Box::pin(writer.commit()).as_mut().poll(cx))
        .expect_err("commit must surface the failed write during settlement");
    assert!(!error.retryable());
    assert_eq!(
        error.commit_error().state(),
        qubit_local_files::outcome::LocalWriteFailureState::NotPublished
    );
    assert!(
        error
            .commit_error()
            .error()
            .to_string()
            .contains("2 bytes written before failure")
    );
    assert_eq!(worker.writes(), b"ab");

    let worker = Arc::new(FakeWorker::with_bytes(b"unconfirmed"));
    worker.panic_next();
    let mut panic_reader = reader(&worker);
    let mut output = [0; 4];
    let error = match poll(|cx| Pin::new(&mut panic_reader).poll_read(cx, &mut output)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected driver panic to become an error, got {other:?}"),
    };
    assert!(error.to_string().contains("Unconfirmed"));

    let worker = Arc::new(FakeWorker::with_bytes(b"unknown"));
    worker.unconfirmed_next();
    let mut lost_reader = reader(&worker);
    let mut output = [0; 4];
    let error = match poll(|cx| Pin::new(&mut lost_reader).poll_read(cx, &mut output)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected lost driver to remain unconfirmed, got {other:?}"),
    };
    assert!(error.to_string().contains("Unconfirmed"));
    assert!(
        matches!(poll(|cx| Pin::new(&mut lost_reader).poll_read(cx, &mut output)), Poll::Ready(Err(error)) if error.to_string().contains("Unconfirmed"))
    );
}

#[test]
fn test_separate_streams_keep_independent_offsets() {
    let first_worker = Arc::new(FakeWorker::with_bytes(b"abc"));
    let second_worker = Arc::new(FakeWorker::with_bytes(b"XYZ"));
    let mut first = reader(&first_worker);
    let mut second = reader(&second_worker);
    let mut a = [0; 1];
    let mut x = [0; 1];
    assert!(matches!(
        poll(|cx| Pin::new(&mut first).poll_read(cx, &mut a)),
        Poll::Ready(Ok(1))
    ));
    assert!(matches!(
        poll(|cx| Pin::new(&mut second).poll_read(cx, &mut x)),
        Poll::Ready(Ok(1))
    ));
    assert!(matches!(
        poll(|cx| Pin::new(&mut first).poll_read(cx, &mut a)),
        Poll::Ready(Ok(1))
    ));
    assert_eq!(&a, b"b");
    assert_eq!(&x, b"X");
}

#[test]
fn test_walker_enforces_batch_limits_and_drop_confirms_worker_close() {
    let oversized = Some((0..129).map(|index| PathBuf::from(format!("entry-{index}"))).collect());
    let worker = Arc::new(FakeWalkWorker::with_batches([oversized]));
    let mut walker = AsyncLocalDirectoryWalker::new(walk_resource(Arc::clone(&worker))).unwrap();
    let error = match poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected batch count limit failure, got {other:?}"),
    };
    assert_eq!(
        error.kind(),
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit
    );
    assert!(error.to_string().contains("128 entry limit"));
    drop(walker);
    assert!(worker.0.lock().unwrap().stopped);

    let worker = Arc::new(FakeWalkWorker::with_batches([Some(vec![PathBuf::from("entry")]), None]));
    worker.0.lock().unwrap().allow_close = false;
    let mut walker = AsyncLocalDirectoryWalker::new(walk_resource(Arc::clone(&worker))).unwrap();
    let observer = walker.close_observer();
    assert!(
        matches!(poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)), Poll::Ready(Ok(Some(batch))) if batch == [PathBuf::from("entry")])
    );
    let calls_before_drop = worker.0.lock().unwrap().calls;
    drop(walker);
    assert!(worker.0.lock().unwrap().stopped);
    assert_eq!(
        worker.0.lock().unwrap().calls,
        calls_before_drop,
        "drop must stop subsequent batches"
    );
    assert!(poll(|cx| Box::pin(observer.wait_closed()).as_mut().poll(cx)).is_pending());
    worker.release_close();
    assert!(matches!(
        poll(|cx| Box::pin(observer.wait_closed()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert!(worker.0.lock().unwrap().closed);

    let long_name = PathBuf::from("x".repeat(256 * 1024 + 1));
    let worker = Arc::new(FakeWalkWorker::with_batches([Some(vec![long_name])]));
    let mut walker = AsyncLocalDirectoryWalker::new(walk_resource(worker)).unwrap();
    let error = match poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected oversized single path to fail, got {other:?}"),
    };
    assert_eq!(
        error.kind(),
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit
    );
    assert!(error.to_string().contains("single walker path"));

    let oversized_batch = (0..128)
        .map(|index| PathBuf::from(format!("entry-{index}-{}", "x".repeat(2048))))
        .collect();
    let worker = Arc::new(FakeWalkWorker::with_batches([Some(oversized_batch)]));
    let mut walker = AsyncLocalDirectoryWalker::new(walk_resource(Arc::clone(&worker))).unwrap();
    let error = match poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected oversized path batch to fail, got {other:?}"),
    };
    assert_eq!(
        error.kind(),
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit
    );
    assert!(error.to_string().contains("256 KiB path budget"));
    assert!(worker.0.lock().unwrap().stopped);
}

#[test]
fn test_walker_terminal_batch_and_close_observer_are_repeatable() {
    let worker = Arc::new(FakeWalkWorker::with_batches([None]));
    let mut walker = AsyncLocalDirectoryWalker::new(walk_resource(Arc::clone(&worker))).unwrap();
    assert!(matches!(
        poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)),
        Poll::Ready(Ok(None))
    ));
    assert!(matches!(
        poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)),
        Poll::Ready(Ok(None))
    ));
    let observer = walker.close_observer();
    assert!(matches!(
        poll(|cx| Box::pin(observer.wait_closed()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        poll(|cx| Box::pin(walker.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        poll(|cx| Box::pin(walker.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));

    let worker = Arc::new(FakeWalkWorker::with_batches([Some(vec![PathBuf::from("entry")])]));
    let mut walker = AsyncLocalDirectoryWalker::new(walk_resource(worker)).unwrap();
    assert!(matches!(
        poll(|cx| Box::pin(walker.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        poll(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)),
        Poll::Ready(Ok(None))
    ));
    let observer = walker.close_observer();
    assert!(matches!(
        poll(|cx| Box::pin(observer.wait_closed()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        poll(|cx| Box::pin(walker.close()).as_mut().poll(cx)),
        Poll::Ready(Ok(()))
    ));
}

#[test]
fn test_settled_walker_can_reopen_immediately() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalListOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("entry"), b"x").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 1,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let mut first_open = filesystem
        .submit_open_walker(
            &manager,
            Default::default(),
            directory.path().to_path_buf(),
            LocalListOptions::default(),
        )
        .unwrap();
    let mut first = match first_open.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(walker) => walker,
        _ => panic!("first walker should open"),
    };
    while poll_worker(|cx| Box::pin(first.next_batch()).as_mut().poll(cx))
        .unwrap()
        .is_some()
    {}
    drop(first);
    first_open.wait_settlement().unwrap();

    let mut second_open = filesystem
        .submit_open_walker(
            &manager,
            Default::default(),
            directory.path().to_path_buf(),
            LocalListOptions::default(),
        )
        .unwrap();
    let second = match second_open.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(walker) => walker,
        _ => panic!("second walker should open after settlement"),
    };
    drop(second);
    second_open.wait_settlement().unwrap();
}
