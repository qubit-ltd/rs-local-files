// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// =============================================================================

#![cfg(feature = "async")]

use std::future::Future;
use std::io;
use std::io::SeekFrom;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

use qubit_local_files::operation::stream::AsyncLocalFileReader;
use qubit_local_files::operation::stream::AsyncLocalFileWriter;
use qubit_local_files::operation::stream::LocalStreamCompletion;
use qubit_local_files::operation::stream::LocalStreamDriver;
use qubit_local_files::operation::stream::LocalStreamRequest;
use qubit_local_files::operation::stream::LocalStreamResource;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::ResourceRef;
use qubit_operation::ResourceScopeOptions;

struct PanickingDriver;

impl LocalStreamDriver for PanickingDriver {
    fn poll_command(&self, _: &mut Context<'_>, _: &LocalStreamRequest) -> Poll<io::Result<LocalStreamCompletion>> {
        panic!("driver poll panic")
    }

    fn request_close(&self, _: u64) {
        panic!("driver close panic")
    }
}

fn resource() -> ResourceRef<LocalStreamResource> {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let mut operation = manager
        .try_submit(Default::default(), |scope| {
            let resources = scope.resource_scope(ResourceScopeOptions { capacity: 1 }).unwrap();
            Ok::<_, ()>(resources.manage(LocalStreamResource::new(PanickingDriver)).unwrap())
        })
        .unwrap();
    match operation.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("resource registration operation failed"),
    }
}

fn poll<T>(future: impl FnOnce(&mut Context<'_>) -> Poll<T>) -> Poll<T> {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    future(&mut cx)
}

#[test]
fn test_reader_claim_is_exclusive_and_panicking_driver_becomes_io_error() {
    let resource = resource();
    let mut reader = AsyncLocalFileReader::new(resource.clone()).unwrap();
    let error = match AsyncLocalFileReader::new(resource) {
        Ok(_) => panic!("second reader unexpectedly claimed the resource"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);

    let error = match poll(|cx| Box::pin(reader.seek(SeekFrom::Start(0))).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected a contained driver panic, got {other:?}"),
    };
    assert_eq!(error.kind(), io::ErrorKind::Other);
}

#[test]
fn test_close_and_drop_contain_driver_panics() {
    let mut reader = AsyncLocalFileReader::new(resource()).unwrap();
    let error = match poll(|cx| Box::pin(reader.close()).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected a contained driver panic, got {other:?}"),
    };
    assert_eq!(error.kind(), io::ErrorKind::Other);
    drop(reader);
}

#[test]
fn test_writer_commit_and_close_contain_driver_panics() {
    let mut writer = AsyncLocalFileWriter::new(resource()).unwrap();
    let error = match poll(|cx| Box::pin(writer.commit()).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected a contained commit driver panic, got {other:?}"),
    };
    assert!(!error.retryable());

    let error = match poll(|cx| Box::pin(writer.close()).as_mut().poll(cx)) {
        Poll::Ready(Err(error)) => error,
        other => panic!("expected a contained close driver panic, got {other:?}"),
    };
    assert_eq!(error.kind(), io::ErrorKind::Other);
}
