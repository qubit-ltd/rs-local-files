// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "async")]

use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::LocalReadRequest;
use qubit_local_files::operation::AsyncLocalFileSystem;
use qubit_local_files::operation::LocalIoExecutor;
use qubit_local_files::operation::complete::LocalCopyRequest;
use qubit_local_files::operation::complete::LocalCreateDirectoryRequest;
use qubit_local_files::operation::complete::LocalDeleteRequest;
use qubit_local_files::operation::complete::LocalReadAtRequest;
use qubit_local_files::operation::complete::LocalReadPrefixRequest;
use qubit_local_files::operation::complete::LocalRenameRequest;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalReadAllOptions;
use qubit_local_files::options::LocalRenameOptions;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Wake;
    use std::task::Waker;
    struct Signal(Mutex<bool>, Condvar);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            *self.0.lock().unwrap() = true;
            self.1.notify_one();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            *self.0.lock().unwrap() = true;
            self.1.notify_one();
        }
    }
    let signal = Arc::new(Signal(Mutex::new(false), Condvar::new()));
    let waker = Waker::from(Arc::clone(&signal));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        let mut ready = signal.0.lock().unwrap();
        while !*ready {
            ready = signal.1.wait(ready).unwrap();
        }
        *ready = false;
    }
}

fn facade(manager: &OperationManager) -> AsyncLocalFileSystem {
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    block_on(AsyncLocalFileSystem::host(executor)).unwrap()
}

#[test]
fn test_complete_read_reports_limit_error_without_returning_truncated_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("payload");
    std::fs::write(&path, b"four").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let mut exact = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: path.clone(),
                read: LocalReadAllOptions::new().with_max_bytes(4),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(exact.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(bytes) if bytes == b"four"));
    let mut operation = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path,
                read: LocalReadAllOptions::new().with_max_bytes(3),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    let outcome = operation.wait_outcome().unwrap();
    assert!(matches!(outcome, qubit_operation::OperationOutcome::Failed(error)
        if error.read_limit_error().is_some()));
}

#[test]
fn test_read_text_preserves_utf8_conversion_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid-utf8");
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let mut operation = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path,
                read: LocalReadAllOptions::new().with_max_bytes(2),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(operation.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error) if error.kind() == qubit_local_files::error::LocalFileErrorKind::InvalidEncoding));
}

#[test]
fn test_complete_reads_cover_success_and_native_open_failures() {
    let directory = tempfile::tempdir().unwrap();
    let text_path = directory.path().join("text");
    std::fs::write(&text_path, b"complete text").unwrap();
    let missing = directory.path().join("missing");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    let mut text = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: text_path,
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        text.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(value) if value == "complete text"
    ));

    let mut read_all = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: missing.clone(),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        read_all.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::NotFound
    ));

    let mut prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path: missing.clone(),
                read: Default::default(),
                max_bytes: 4,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        prefix.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::NotFound
    ));

    let mut read_at = filesystem
        .submit_read_at(
            &manager,
            Default::default(),
            LocalReadAtRequest {
                path: missing,
                read: Default::default(),
                offset: SeekFrom::Start(0),
                max_bytes: 4,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        read_at.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::NotFound
    ));
}

#[test]
fn test_read_text_preserves_native_open_failure() {
    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let mut operation = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: directory.path().join("missing-text"),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;

    assert!(matches!(
        operation.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::NotFound
    ));
}

#[test]
fn test_invalid_text_keeps_its_owned_byte_permit_until_the_outcome_is_taken() {
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("invalid-utf8");
    let valid = directory.path().join("valid");
    std::fs::write(&invalid, [0xff, b'a', b'b', b'c']).unwrap();
    std::fs::write(&valid, b"data").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = LocalExecutionConfig {
        max_owned_bytes: 4,
        max_open_handles: 2,
        ..LocalExecutionConfig::default()
    };
    let executor = LocalIoExecutor::new(manager.clone(), config).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let mut text = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: invalid,
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;

    text.wait_settlement().unwrap();
    let mut blocked = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: valid.clone(),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        blocked.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(_)
    ));

    assert!(matches!(text.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::InvalidEncoding));
    let mut available = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: valid,
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(available.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(bytes) if bytes == b"data"));
}

#[test]
fn test_read_at_seeks_and_reads_the_same_opened_object() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("payload");
    std::fs::write(&path, b"012345").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let request = LocalReadAtRequest {
        path,
        read: Default::default(),
        offset: SeekFrom::Start(2),
        max_bytes: 2,
    };
    let mut operation = filesystem
        .submit_read_at(&manager, Default::default(), request, Default::default())
        .unwrap()
        .0;
    assert!(matches!(operation.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(bytes) if bytes == b"23"));
}

#[test]
fn test_metadata_and_prefix_expose_complete_file_observations() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abcdef").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    let mut metadata = filesystem
        .submit_metadata(&manager, Default::default(), PathBuf::from(&path), Default::default())
        .unwrap()
        .0;
    assert!(matches!(metadata.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(value) if value.len() == 6));

    let mut prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path,
                read: Default::default(),
                max_bytes: 3,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(prefix.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(value)
            if value.bytes == b"abc" && !value.complete));

    let mut short_prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path: directory.path().join("payload"),
                read: Default::default(),
                max_bytes: 8,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(short_prefix.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(value)
            if value.bytes == b"abcdef" && value.complete));
}

#[test]
fn test_prefix_bound_larger_than_executor_budget_only_charges_retained_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("short");
    std::fs::write(&path, b"short").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = LocalExecutionConfig {
        max_owned_bytes: 16,
        ..LocalExecutionConfig::default()
    };
    let executor = LocalIoExecutor::new(manager.clone(), config).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let mut operation = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path,
                read: Default::default(),
                max_bytes: 1024,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(operation.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(prefix)
            if prefix.bytes == b"short" && prefix.complete));
}

#[test]
fn test_complete_read_rejections_return_the_original_request_for_invalid_limits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("payload");
    std::fs::write(&path, b"payload").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = LocalExecutionConfig {
        max_owned_bytes: 8,
        ..LocalExecutionConfig::default()
    };
    let executor = LocalIoExecutor::new(manager.clone(), config).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let limits = qubit_local_files::LocalOperationOptions {
        max_owned_bytes: Some(9),
        ..Default::default()
    };

    let read_all = LocalReadRequest {
        path: path.clone(),
        read: Default::default(),
    };
    let error = match filesystem.submit_read_all(&manager, Default::default(), read_all, limits) {
        Ok(_) => panic!("read-all must reject a per-operation limit above the executor limit"),
        Err(error) => error,
    };
    assert_eq!(
        error.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    assert_eq!(error.into_request().path, path);

    let path = directory.path().join("payload");
    let read_prefix = LocalReadPrefixRequest {
        path: path.clone(),
        read: Default::default(),
        max_bytes: 4,
    };
    let error = match filesystem.submit_read_prefix(&manager, Default::default(), read_prefix, limits) {
        Ok(_) => panic!("prefix must reject a per-operation limit above the executor limit"),
        Err(error) => error,
    };
    assert_eq!(
        error.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    assert_eq!(error.into_request().path, path);

    let path = directory.path().join("payload");
    let read_at = LocalReadAtRequest {
        path: path.clone(),
        read: Default::default(),
        offset: SeekFrom::Start(0),
        max_bytes: 4,
    };
    let error = match filesystem.submit_read_at(&manager, Default::default(), read_at, limits) {
        Ok(_) => panic!("read-at must reject a per-operation limit above the executor limit"),
        Err(error) => error,
    };
    assert_eq!(
        error.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    assert_eq!(error.into_request().path, path);
}

#[test]
fn test_operation_handle_limits_reserve_the_required_native_handle_count() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::write(&source, b"copy").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    let copy_request = LocalCopyRequest {
        source: source.clone(),
        destination: destination.clone(),
        options: LocalCopyOptions::new(),
    };
    let rejected = match filesystem.submit_copy(
        &manager,
        Default::default(),
        copy_request,
        qubit_local_files::LocalOperationOptions {
            max_open_handles: Some(2),
            ..Default::default()
        },
    ) {
        Ok(_) => {
            panic!("copy admission must reserve source, destination, and source-directory handles")
        }
        Err(error) => error,
    };
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    assert_eq!(rejected.into_request().source, source);

    let (mut metadata, _) = filesystem
        .submit_metadata(
            &manager,
            Default::default(),
            source.clone(),
            qubit_local_files::LocalOperationOptions {
                max_open_handles: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(matches!(metadata.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(metadata) if metadata.len() == 4));

    let (mut copy, _) = filesystem
        .submit_copy(
            &manager,
            Default::default(),
            LocalCopyRequest {
                source,
                destination: destination.clone(),
                options: LocalCopyOptions::new(),
            },
            qubit_local_files::LocalOperationOptions {
                max_open_handles: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(matches!(
        copy.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
}

#[test]
fn test_copy_rename_create_and_delete_retain_structured_results_and_facts() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let copied = directory.path().join("copied");
    let renamed = directory.path().join("renamed");
    let created = directory.path().join("created-dir");
    std::fs::write(&source, b"payload").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    let (mut copy, copy_facts) = filesystem
        .submit_copy(
            &manager,
            Default::default(),
            LocalCopyRequest {
                source: source.clone(),
                destination: copied.clone(),
                options: LocalCopyOptions::new(),
            },
            Default::default(),
        )
        .unwrap();
    assert!(matches!(copy.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result) if result.outcome.stats().bytes() == 7));
    assert_eq!(copy_facts.snapshot().modified_bytes, 7);
    assert_eq!(std::fs::read(&copied).unwrap(), b"payload");

    let (mut rename, rename_facts) = filesystem
        .submit_rename(
            &manager,
            Default::default(),
            LocalRenameRequest {
                source: copied.clone(),
                destination: renamed.clone(),
                options: LocalRenameOptions::new(),
            },
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        rename.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
    assert_eq!(
        rename_facts.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    assert!(!copied.exists());
    assert!(renamed.exists());

    let (mut delete, delete_facts) = filesystem
        .submit_delete(
            &manager,
            Default::default(),
            LocalDeleteRequest {
                path: renamed.clone(),
                options: LocalDeleteOptions::new(),
                directory: false,
            },
            Default::default(),
        )
        .unwrap();
    assert!(matches!(delete.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result) if result.outcome.deleted()));
    assert_eq!(delete_facts.snapshot().modified_count, 1);
    assert!(!renamed.exists());

    let (mut create, create_facts) = filesystem
        .submit_create_directory(
            &manager,
            Default::default(),
            qubit_local_files::operation::complete::LocalCreateDirectoryRequest {
                path: created.clone(),
                options: Default::default(),
            },
            Default::default(),
        )
        .unwrap();
    assert!(matches!(create.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result) if result.outcome.created()));
    assert_eq!(create_facts.snapshot().modified_count, 1);
    assert!(created.is_dir());

    let (mut missing_copy, missing_copy_facts) = filesystem
        .submit_copy(
            &manager,
            Default::default(),
            LocalCopyRequest {
                source: directory.path().join("missing-source"),
                destination: directory.path().join("missing-copy"),
                options: LocalCopyOptions::new(),
            },
            Default::default(),
        )
        .unwrap();
    match missing_copy.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(failure) => {
            assert_eq!(
                failure.state(),
                qubit_local_files::outcome::LocalCopyFailureState::Unchanged
            );
        }
        other => panic!("missing-source copy must retain its typed failure: {other:?}"),
    }
    assert_eq!(
        missing_copy_facts.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );

    let (mut missing_rename, missing_rename_facts) = filesystem
        .submit_rename(
            &manager,
            Default::default(),
            LocalRenameRequest {
                source: directory.path().join("missing-rename-source"),
                destination: directory.path().join("missing-rename-target"),
                options: LocalRenameOptions::new(),
            },
            Default::default(),
        )
        .unwrap();
    match missing_rename.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(failure) => {
            assert_eq!(
                failure.state(),
                qubit_local_files::outcome::LocalRenameFailureState::Unchanged
            );
        }
        other => panic!("missing-source rename must retain its typed failure: {other:?}"),
    }
    assert_eq!(
        missing_rename_facts.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
}

#[test]
fn test_complete_operation_edge_results_and_failures() {
    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    // A seek beyond EOF is a successful empty read.
    let empty_path = directory.path().join("empty-read");
    std::fs::write(&empty_path, b"x").unwrap();
    let mut read_at = filesystem
        .submit_read_at(
            &manager,
            Default::default(),
            LocalReadAtRequest {
                path: empty_path,
                read: Default::default(),
                offset: SeekFrom::Start(2),
                max_bytes: 2,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(read_at.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(bytes) if bytes.is_empty()));

    // Prefix reads probe one byte past the requested prefix. This
    // deterministically exercises the executor byte-budget failure while
    // retaining no partial output.
    let prefix_path = directory.path().join("prefix-budget");
    std::fs::write(&prefix_path, b"ab").unwrap();
    let mut prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path: prefix_path,
                read: Default::default(),
                max_bytes: 1,
            },
            qubit_local_files::LocalOperationOptions {
                max_owned_bytes: Some(1),
                ..Default::default()
            },
        )
        .unwrap()
        .0;
    assert!(matches!(prefix.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::Io));

    // Creating an already existing directory succeeds with created=false.
    let existing = directory.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    let mut create = filesystem
        .submit_create_directory(
            &manager,
            Default::default(),
            LocalCreateDirectoryRequest {
                path: existing.clone(),
                options: qubit_local_files::options::LocalCreateDirectoryOptions::new().with_exists_ok(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(create.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result) if !result.outcome.created()));

    // A file blocking a parent component makes directory creation fail.
    let blocker = directory.path().join("blocker");
    std::fs::write(&blocker, b"file").unwrap();
    let mut failed_create = filesystem
        .submit_create_directory(
            &manager,
            Default::default(),
            LocalCreateDirectoryRequest {
                path: blocker.join("child"),
                options: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        failed_create.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(_)
    ));

    // Recursive deletion succeeds but cannot report an exact entry count.
    let tree = directory.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("child"), b"x").unwrap();
    let mut delete = filesystem
        .submit_delete(
            &manager,
            Default::default(),
            LocalDeleteRequest {
                path: tree.clone(),
                options: LocalDeleteOptions::new().with_recursive(),
                directory: true,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(delete.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result)
            if result.outcome.deleted() && result.deleted_entries.is_none()));
    assert!(!tree.exists());
}

#[test]
fn test_rooted_complete_mutations_bind_relative_paths_to_captured_root() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source"), b"rooted payload").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, root.clone())).unwrap();

    let mut copy = filesystem
        .submit_copy(
            &manager,
            Default::default(),
            LocalCopyRequest {
                source: PathBuf::from("source"),
                destination: PathBuf::from("copied"),
                options: LocalCopyOptions::new(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        copy.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result)
            if result.source.as_path() == Path::new("source")
                && result.destination.as_path() == Path::new("copied")
    ));
    assert_eq!(std::fs::read(root.join("copied")).unwrap(), b"rooted payload");
    assert!(!scratch.path().join("copied").exists());

    let mut rename = filesystem
        .submit_rename(
            &manager,
            Default::default(),
            LocalRenameRequest {
                source: PathBuf::from("copied"),
                destination: PathBuf::from("renamed"),
                options: LocalRenameOptions::new(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        rename.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result)
            if result.source.as_path() == Path::new("copied")
                && result.destination.as_path() == Path::new("renamed")
    ));
    assert!(!root.join("copied").exists());
    assert_eq!(std::fs::read(root.join("renamed")).unwrap(), b"rooted payload");

    let mut create = filesystem
        .submit_create_directory(
            &manager,
            Default::default(),
            LocalCreateDirectoryRequest {
                path: PathBuf::from("created"),
                options: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        create.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result) if result.outcome.created()
    ));
    assert!(root.join("created").is_dir());
    assert!(!scratch.path().join("created").exists());

    let mut delete = filesystem
        .submit_delete(
            &manager,
            Default::default(),
            LocalDeleteRequest {
                path: PathBuf::from("renamed"),
                options: LocalDeleteOptions::new(),
                directory: false,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        delete.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result) if result.outcome.deleted()
    ));
    assert!(!root.join("renamed").exists());
}

#[test]
fn test_delete_reports_missing_file_and_directory_errors() {
    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    let mut missing_file = filesystem
        .submit_delete(
            &manager,
            Default::default(),
            LocalDeleteRequest {
                path: directory.path().join("missing-file"),
                options: LocalDeleteOptions::new(),
                directory: false,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        missing_file.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::NotFound
    ));

    let mut missing_directory = filesystem
        .submit_delete(
            &manager,
            Default::default(),
            LocalDeleteRequest {
                path: directory.path().join("missing-directory"),
                options: LocalDeleteOptions::new(),
                directory: true,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(
        missing_directory.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Failed(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::NotFound
    ));
}
