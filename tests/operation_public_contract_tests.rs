// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "async")]

use std::sync::Arc;

use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::LocalExecutorCreateError;
use qubit_local_files::LocalFactObserver;
use qubit_local_files::LocalFactSnapshot;
use qubit_local_files::LocalOperation;
use qubit_local_files::LocalOperationOptions;
use qubit_local_files::LocalReadRequest;
use qubit_local_files::LocalSubmitError;
use qubit_local_files::LocalSubmitRejection;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::PreparedExternalOperation;

struct OpaqueRequest;

fn assert_send_sync_static<T: Send + Sync + 'static>() {}
fn assert_copy<T: Copy>() {}

#[test]
fn test_lf1_types_are_available_from_the_crate_root() {
    assert_send_sync_static::<LocalFactObserver>();
    assert_send_sync_static::<LocalFactSnapshot>();
    assert_send_sync_static::<LocalExecutionConfig>();
    assert_send_sync_static::<LocalExecutorCreateError>();
    assert_send_sync_static::<LocalOperationOptions>();
    assert_send_sync_static::<LocalReadRequest>();
    assert_copy::<LocalFactSnapshot>();
    let _: Option<LocalOperation<Vec<u8>>> = None;
    let _: Option<LocalSubmitRejection> = None;
}

#[test]
fn test_rejected_request_type_accepts_an_opaque_non_clone_non_debug_payload() {
    fn recover(error: LocalSubmitError<OpaqueRequest>) -> OpaqueRequest {
        error.into_request()
    }

    let _recover = recover;
}

#[cfg(feature = "async")]
#[test]
fn test_lf8_async_surface_is_available_from_the_crate_root() {
    use qubit_local_files::AsyncLocalDirectoryWalker;
    use qubit_local_files::AsyncLocalFileReader;
    use qubit_local_files::AsyncLocalFileSystem;
    use qubit_local_files::AsyncLocalFileWriter;
    use qubit_local_files::LocalReadExport;
    use qubit_local_files::LocalReadLease;
    use qubit_local_files::LocalReadResource;
    use qubit_local_files::LocalStageOptions;
    use qubit_local_files::LocalStreamCommand;
    use qubit_local_files::LocalStreamResource;
    use qubit_local_files::LocalTempFileAccess;
    use qubit_local_files::LocalTempLifecycleCommand;
    use qubit_local_files::LocalWalkResource;
    use qubit_local_files::LocalWriterCommitRequest;
    use qubit_local_files::stage_bytes;
    use qubit_local_files::stage_stream;
    use qubit_local_files::submit_temp_directory;
    use qubit_local_files::submit_temp_file;

    let _: Option<AsyncLocalDirectoryWalker> = None;
    let _: Option<AsyncLocalFileReader> = None;
    let _: Option<AsyncLocalFileSystem> = None;
    let _: Option<AsyncLocalFileWriter> = None;
    let _: Option<LocalReadExport> = None;
    let _: Option<LocalReadLease> = None;
    let _: Option<LocalReadResource> = None;
    let _: Option<LocalStageOptions> = None;
    let _: Option<LocalStreamCommand> = None;
    let _: Option<LocalStreamResource> = None;
    let _: Option<LocalTempFileAccess> = None;
    let _: Option<LocalTempLifecycleCommand> = None;
    let _: Option<LocalWriterCommitRequest> = None;
    let _: Option<LocalWalkResource> = None;
    let _ = (stage_bytes, submit_temp_directory, submit_temp_file);
    let _ = stage_stream::<AsyncLocalFileReader>;
}

#[cfg(all(feature = "async", feature = "digest"))]
#[test]
fn test_lf8_digest_surface_is_available_from_the_crate_root() {
    use qubit_local_files::LocalConditionalWriterCommandError;
    use qubit_local_files::LocalConditionalWriterCommitRequest;
    let _: Option<LocalConditionalWriterCommandError<sha2::Sha256>> = None;
    let _: Option<LocalConditionalWriterCommitRequest<sha2::Sha256>> = None;
}

// This generic external test compiles against only the dependency's public
// crate root. It deliberately does not import downstream-private adapters.
#[allow(dead_code)]
fn submit_prepared<T, E, P>(manager: &OperationManager, prepared: Arc<P>)
where
    T: Send + 'static,
    E: Send + 'static,
    P: PreparedExternalOperation<T, E>,
{
    let _ = manager.try_submit_external(OperationOptions::default(), prepared);
}
