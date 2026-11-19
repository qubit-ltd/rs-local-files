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

#[derive(Default)]
struct FakeState {
    bytes: Vec<u8>,
    position: usize,
    writes: Vec<u8>,
    hold_next: bool,
    held: Vec<u64>,
    wake: Option<Waker>,
    close_count: usize,
    close_requested: bool,
    partial_write: Option<usize>,
    fail_after_write: Option<usize>,
    write_calls: usize,
    panic_next: bool,
    unconfirmed_next: bool,
    fail_seek: bool,
    oversized_read: Option<Vec<u8>>,
    hold_read_after_execute: bool,
    completed_reads: HashMap<u64, Vec<u8>>,
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
            LocalStreamCommand::Close | LocalStreamCommand::Abort => {
                state.close_count += 1;
                Ok(LocalStreamCompletion::Closed)
            }
            LocalStreamCommand::Commit => Ok(LocalStreamCompletion::Closed),
        };
        Poll::Ready(result)
    }

    fn request_close(&self, _request_id: u64) {
        self.0.lock().unwrap().close_requested = true;
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

#[derive(Default)]
struct FakeWalkState {
    batches: VecDeque<Option<Vec<PathBuf>>>,
    calls: usize,
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
}

impl LocalWalkDriver for FakeWalkWorker {
    fn poll_batch(
        &self,
        _cx: &mut Context<'_>,
        _request_id: u64,
        _max_entries: usize,
        _max_bytes: usize,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        let mut state = self.0.lock().unwrap();
        state.calls += 1;
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
}

fn poll<T>(future: impl FnOnce(&mut Context<'_>) -> Poll<T>) -> Poll<T> {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    future(&mut cx)
}

fn poll_worker<T>(mut future: impl FnMut(&mut Context<'_>) -> Poll<T>) -> T {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    for _ in 0..100_000 {
        if let Poll::Ready(value) = future(&mut cx) {
            return value;
        }
        std::thread::yield_now();
    }
    panic!("worker-backed operation did not complete");
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
fn test_dropped_real_worker_writer_releases_single_handle_for_reopen() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

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
    assert_eq!(
        poll_worker(|cx| Pin::new(&mut abandoned).poll_write(cx, b"discarded")).unwrap(),
        9
    );
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
        .unwrap();
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

#[cfg(feature = "test-support")]
#[test]
fn test_recursive_worker_walker_returns_native_entry_error() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("nested")).unwrap();
    std::fs::write(directory.path().join("nested/entry"), b"").unwrap();
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

    let _fault = install_test_fault_shared("walker-entry").unwrap();
    let error = poll_worker(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)).unwrap_err();
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::Io);
    assert!(error.to_string().contains("injected walker directory entry failure"));
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
