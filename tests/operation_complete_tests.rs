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
use qubit_local_files::error::LocalFileErrorSource;
use qubit_local_files::operation::AsyncLocalFileSystem;
use qubit_local_files::operation::LocalIoExecutor;
use qubit_local_files::operation::complete::LocalCopyRequest;
use qubit_local_files::operation::complete::LocalCreateDirectoryRequest;
use qubit_local_files::operation::complete::LocalDeleteRequest;
use qubit_local_files::operation::complete::LocalReadAtRequest;
use qubit_local_files::operation::complete::LocalReadPrefixRequest;
use qubit_local_files::operation::complete::LocalRenameRequest;
#[cfg(unix)]
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
#[cfg(unix)]
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::options::LocalDeleteOptions;
#[cfg(unix)]
use qubit_local_files::options::LocalMetadataPreservePolicy;
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
fn test_async_complete_reads_preserve_the_dominant_limit_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("payload");
    std::fs::write(&path, b"12345").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_owned_bytes: 8,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let memory_limit = qubit_local_files::LocalOperationOptions {
        max_owned_bytes: Some(4),
        ..Default::default()
    };

    macro_rules! assert_owned_bytes_limit {
        ($operation:expr) => {{
            match $operation.wait_outcome().unwrap() {
                qubit_operation::OperationOutcome::Failed(error) => {
                    let limit = error.resource_limit_error().expect("owned-byte limit facts");
                    assert_eq!(error.path(), Some(path.as_path()));
                    assert_eq!(
                        limit.resource(),
                        qubit_local_files::error::LocalResourceKind::OwnedBytes
                    );
                    assert_eq!((limit.limit(), limit.remaining(), limit.requested()), (4, 4, 5));
                    assert_eq!(limit.observed(), Some(5));
                    assert!(error.read_limit_error().is_none());
                }
                other => panic!("memory limit must fail as OwnedBytes: {other:?}"),
            }
        }};
    }

    let mut all = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: path.clone(),
                read: LocalReadAllOptions::new().with_max_bytes(8),
            },
            memory_limit,
        )
        .unwrap()
        .0;
    assert_owned_bytes_limit!(all);

    let mut text = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: path.clone(),
                read: Default::default(),
            },
            memory_limit,
        )
        .unwrap()
        .0;
    assert_owned_bytes_limit!(text);

    let mut observed = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: path.clone(),
                read: LocalReadAllOptions::new().with_max_bytes(8),
            },
            memory_limit,
        )
        .unwrap()
        .0;
    assert_owned_bytes_limit!(observed);

    let mut explicit = filesystem
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
    match explicit.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(path.as_path()));
            let limit = error.read_limit_error().expect("explicit content limit");
            assert_eq!((limit.limit(), limit.observed()), (4, 5));
            assert!(error.resource_limit_error().is_none());
        }
        other => panic!("explicit content limit must fail as read-limit: {other:?}"),
    }

    let mut equal = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path,
                read: LocalReadAllOptions::new().with_max_bytes(4),
            },
            qubit_local_files::LocalOperationOptions {
                max_owned_bytes: Some(4),
                ..Default::default()
            },
        )
        .unwrap()
        .0;
    assert!(
        matches!(equal.wait_outcome().unwrap(), qubit_operation::OperationOutcome::Failed(error)
        if error.read_limit_error().is_some() && error.resource_limit_error().is_none())
    );
}

#[test]
fn test_rooted_observer_read_limits_keep_bound_path_and_pwd_context() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("preflight"), b"12345").unwrap();
    std::fs::write(root.join("held"), b"four").unwrap();
    std::fs::write(root.join("shared"), b"x").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_owned_bytes: 4,
            worker_count: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, root)).unwrap();
    let memory_limit = qubit_local_files::LocalOperationOptions {
        max_owned_bytes: Some(4),
        ..Default::default()
    };

    let mut preflight = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("preflight"),
                read: LocalReadAllOptions::new().with_max_bytes(8),
            },
            memory_limit,
        )
        .unwrap()
        .0;
    match preflight.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(Path::new("/preflight")));
            assert_eq!(error.current_directory(), Some(Path::new("/")));
            assert_eq!(format!("{:?}", error.operation()), "Read");
            let limit = error.resource_limit_error().unwrap();
            assert_eq!((limit.limit(), limit.remaining(), limit.requested()), (4, 4, 5));
        }
        other => panic!("initial length over the memory limit must fail: {other:?}"),
    }

    let held = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("held"),
                read: LocalReadAllOptions::new().with_max_bytes(8),
            },
            memory_limit,
        )
        .unwrap()
        .0;
    held.wait_settlement().unwrap();
    let mut shared = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("shared"),
                read: LocalReadAllOptions::new().with_max_bytes(8),
            },
            memory_limit,
        )
        .unwrap()
        .0;
    match shared.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(Path::new("/shared")));
            assert_eq!(error.current_directory(), Some(Path::new("/")));
            assert_eq!(format!("{:?}", error.operation()), "Read");
            assert!(error.resource_limit_error().is_some());
        }
        other => panic!("the occupied shared pool must reject the read: {other:?}"),
    }
}

#[test]
fn test_rooted_complete_read_errors_keep_virtual_path_and_pwd() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("limited"), b"12345").unwrap();
    std::fs::write(root.join("invalid"), [0xff]).unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_owned_bytes: 8,
            ..Default::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, root)).unwrap();

    let mut limited = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("limited"),
                read: LocalReadAllOptions::new().with_max_bytes(4),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    match limited.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(Path::new("/limited")));
            assert_eq!(error.current_directory(), Some(Path::new("/")));
            let limit = error.read_limit_error().unwrap();
            assert_eq!((limit.limit(), limit.observed()), (4, 5));
        }
        other => panic!("explicit limit must fail: {other:?}"),
    }

    let mut memory_limited = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("limited"),
                read: Default::default(),
            },
            qubit_local_files::LocalOperationOptions {
                max_owned_bytes: Some(4),
                ..Default::default()
            },
        )
        .unwrap()
        .0;
    match memory_limited.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(Path::new("/limited")));
            assert_eq!(error.current_directory(), Some(Path::new("/")));
            let limit = error.resource_limit_error().expect("owned-byte limit facts");
            assert_eq!(
                limit.resource(),
                qubit_local_files::error::LocalResourceKind::OwnedBytes
            );
            assert_eq!((limit.limit(), limit.remaining(), limit.requested()), (4, 4, 5));
            assert_eq!(limit.observed(), Some(5));
            assert!(error.read_limit_error().is_none());
        }
        other => panic!("owned-byte limit must fail: {other:?}"),
    }

    let mut invalid = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("invalid"),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    match invalid.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(Path::new("/invalid")));
            assert_eq!(error.current_directory(), Some(Path::new("/")));
            assert!(matches!(
                error.into_source(),
                Some(LocalFileErrorSource::InvalidEncoding(_))
            ));
        }
        other => panic!("invalid UTF-8 must fail: {other:?}"),
    }
}

#[test]
fn test_rooted_prefix_and_position_read_budget_errors_keep_virtual_path_and_pwd() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("held"), b"four").unwrap();
    std::fs::write(root.join("prefix"), b"x").unwrap();
    std::fs::write(root.join("position"), b"y").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_owned_bytes: 4,
            worker_count: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, root)).unwrap();
    let held = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("held"),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    held.wait_settlement().unwrap();

    let mut prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path: PathBuf::from("prefix"),
                read: Default::default(),
                max_bytes: 1,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    let mut read_at = filesystem
        .submit_read_at(
            &manager,
            Default::default(),
            LocalReadAtRequest {
                path: PathBuf::from("position"),
                read: Default::default(),
                offset: SeekFrom::Start(0),
                max_bytes: 1,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    match prefix.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(Path::new("/prefix")));
            assert_eq!(error.current_directory(), Some(Path::new("/")));
            assert_eq!(
                error.resource_limit_error().unwrap().resource(),
                qubit_local_files::error::LocalResourceKind::OwnedBytes
            );
        }
        other => panic!("the held result must exhaust shared capacity: {other:?}"),
    }
    match read_at.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            assert_eq!(error.path(), Some(Path::new("/position")));
            assert_eq!(error.current_directory(), Some(Path::new("/")));
            assert_eq!(
                error.resource_limit_error().unwrap().resource(),
                qubit_local_files::error::LocalResourceKind::OwnedBytes
            );
        }
        other => panic!("the held result must exhaust shared capacity: {other:?}"),
    }
}

#[test]
fn test_complete_text_read_succeeds_with_explicit_and_inherited_limits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("text");
    std::fs::write(&path, b"valid text").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    for read in [
        LocalReadAllOptions::new().with_max_bytes(10),
        LocalReadAllOptions::new(),
    ] {
        let mut operation = filesystem
            .submit_read_text(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: path.clone(),
                    read,
                },
                Default::default(),
            )
            .unwrap()
            .0;
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            qubit_operation::OperationOutcome::Succeeded(text) if text == "valid text"
        ));
    }
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

#[cfg(unix)]
#[test]
fn test_complete_reads_apply_outer_symlink_policy_to_inherited_defaults() {
    use std::os::unix::fs::symlink;
    use std::time::Duration;

    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalReadOptions;
    use qubit_local_files::policy::LocalSymlinkPolicy as Policy;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    std::fs::write(&target, b"payload").unwrap();
    symlink(&target, &link).unwrap();

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let mut local = LocalFileSystem::host().unwrap();
    local
        .set_default_read_options(
            LocalReadOptions::new()
                .with_open_retry_timeout(Duration::from_millis(25))
                .with_symlink_policy(Policy::FollowWithinScope),
        )
        .unwrap();
    let filesystem = local.into_async(executor).unwrap();

    let mut read_all = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: link.clone(),
                read: qubit_local_files::options::LocalReadAllOptions::new().with_symlink_policy(Policy::Reject),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(read_all.wait_outcome().unwrap(),
        OperationOutcome::Failed(error) if error.kind() == qubit_local_files::error::LocalFileErrorKind::Unsupported));

    let mut read_text = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: link,
                read: qubit_local_files::options::LocalReadAllOptions::new().with_symlink_policy(Policy::Reject),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(read_text.wait_outcome().unwrap(),
        OperationOutcome::Failed(error) if error.kind() == qubit_local_files::error::LocalFileErrorKind::Unsupported));
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
    match blocked.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => {
            let limit = error.resource_limit_error().expect("shared byte-pool facts");
            assert_eq!(
                error.kind(),
                qubit_local_files::error::LocalFileErrorKind::ResourceLimit
            );
            assert_eq!(error.path(), Some(valid.as_path()));
            assert_eq!(
                limit.resource(),
                qubit_local_files::error::LocalResourceKind::OwnedBytes
            );
            assert_eq!((limit.limit(), limit.remaining(), limit.requested()), (4, 0, 4));
        }
        other => panic!("shared byte-pool exhaustion must reject the read: {other:?}"),
    }

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
fn test_reads_reserve_only_bytes_that_are_actually_retained() {
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    let held_path = directory.path().join("held");
    let short_path = directory.path().join("short");
    std::fs::write(&held_path, b"four").unwrap();
    std::fs::write(&short_path, b"xy").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_owned_bytes: 8,
            ..Default::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let mut held = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: held_path,
                read: LocalReadAllOptions::new().with_max_bytes(4),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(held.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes == b"four"));

    let mut read_all = filesystem
        .submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: short_path.clone(),
                read: LocalReadAllOptions::new().with_max_bytes(2),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(read_all.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes == b"xy"));
    drop(read_all);

    let mut read_text = filesystem
        .submit_read_text(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: short_path.clone(),
                read: LocalReadAllOptions::new().with_max_bytes(2),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(read_text.wait_outcome().unwrap(), OperationOutcome::Succeeded(text) if text == "xy"));
    drop(read_text);

    let mut observed = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: short_path.clone(),
                read: LocalReadAllOptions::new().with_max_bytes(2),
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(observed.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes == b"xy"));
    drop(observed);

    let mut prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path: short_path.clone(),
                read: Default::default(),
                max_bytes: 2,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(
        matches!(prefix.wait_outcome().unwrap(), OperationOutcome::Succeeded(value) if value.bytes == b"xy" && value.complete)
    );
    drop(prefix);

    let mut read_at = filesystem
        .submit_read_at(
            &manager,
            Default::default(),
            LocalReadAtRequest {
                path: short_path,
                read: Default::default(),
                offset: SeekFrom::Start(0),
                max_bytes: 2,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(read_at.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes == b"xy"));
    drop(read_at);
    drop(held);
}

#[test]
fn test_prefix_probe_does_not_consume_owned_byte_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abc").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_owned_bytes: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let mut operation = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path,
                read: Default::default(),
                max_bytes: 1,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(operation.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(prefix)
            if prefix.bytes == b"a" && !prefix.complete));
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
fn test_copy_facts_only_report_actual_publication() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::write(&source, b"original").unwrap();
    std::fs::write(&destination, b"keep").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    let (mut copy, facts) = filesystem
        .submit_copy(
            &manager,
            Default::default(),
            LocalCopyRequest {
                source,
                destination: destination.clone(),
                options: LocalCopyOptions::new()
                    .with_conflict(qubit_local_files::options::LocalCopyConflictPolicy::Skip),
            },
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        copy.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(result)
            if result.outcome.stats().skipped() == 1
                && result.outcome.stats().files() == 0
    ));
    let snapshot = facts.snapshot();
    assert_eq!(
        snapshot.publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
    assert_eq!(snapshot.modified_count, 0);
    assert_eq!(snapshot.modified_bytes, 0);
    assert_eq!(std::fs::read(destination).unwrap(), b"keep");
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

    // The prefix probe is held in a stack byte and is not part of the result
    // budget, so one retained byte still succeeds with a truncated prefix.
    let prefix_path = directory.path().join("prefix-budget");
    std::fs::write(&prefix_path, b"ab").unwrap();
    let mut prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path: prefix_path.clone(),
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
        qubit_operation::OperationOutcome::Succeeded(value)
            if value.bytes == b"a" && !value.complete));

    let mut empty_prefix = filesystem
        .submit_read_prefix(
            &manager,
            Default::default(),
            LocalReadPrefixRequest {
                path: prefix_path,
                read: Default::default(),
                max_bytes: 0,
            },
            Default::default(),
        )
        .unwrap()
        .0;
    assert!(matches!(empty_prefix.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(value)
            if value.bytes.is_empty() && !value.complete));

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
fn test_recursive_missing_ok_delete_reports_known_zero_for_host_and_rooted() {
    for rooted in [false, true] {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = if rooted {
            block_on(AsyncLocalFileSystem::rooted(executor, root.clone())).unwrap()
        } else {
            block_on(AsyncLocalFileSystem::host(executor)).unwrap()
        };
        let missing = if rooted {
            PathBuf::from("missing-tree")
        } else {
            root.join("missing-tree")
        };
        let (mut delete, facts) = filesystem
            .submit_delete(
                &manager,
                Default::default(),
                LocalDeleteRequest {
                    path: missing,
                    options: LocalDeleteOptions::new().with_recursive().with_missing_ok(),
                    directory: true,
                },
                Default::default(),
            )
            .unwrap();
        match delete.wait_outcome().unwrap() {
            qubit_operation::OperationOutcome::Succeeded(result) => {
                assert!(!result.outcome.deleted());
                assert_eq!(Some(0), result.deleted_entries);
            }
            other => panic!("recursive missing_ok deletion should succeed: {other:?}"),
        }
        let snapshot = facts.snapshot();
        assert_eq!(0, snapshot.modified_count);
        assert_eq!(
            qubit_local_files::LocalPublicationState::NotPublished,
            snapshot.publication
        );
        assert!(!snapshot.range_unknown);
    }
}

#[cfg(unix)]
#[test]
fn test_async_copy_records_permission_only_directory_modification() {
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    for rooted in [false, true] {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().join("root");
        let source = root.join("source");
        let target = root.join("target");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("leaf"), b"source").unwrap();
        std::fs::write(target.join("leaf"), b"existing").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o500)).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = if rooted {
            block_on(AsyncLocalFileSystem::rooted(executor, root.clone())).unwrap()
        } else {
            block_on(AsyncLocalFileSystem::host(executor)).unwrap()
        };
        let request_path = |path: &Path, relative: &str| {
            if rooted {
                PathBuf::from(relative)
            } else {
                path.to_path_buf()
            }
        };
        let options = LocalCopyOptions::new()
            .with_tree_source()
            .with_conflict(LocalCopyConflictPolicy::Skip)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Skip)
            .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions);
        let (mut copy, facts) = filesystem
            .submit_copy(
                &manager,
                Default::default(),
                LocalCopyRequest {
                    source: request_path(&source, "source"),
                    destination: request_path(&target, "target"),
                    options,
                },
                Default::default(),
            )
            .unwrap();
        let result = match copy.wait_outcome().unwrap() {
            qubit_operation::OperationOutcome::Succeeded(result) => result,
            other => panic!("directory metadata copy should succeed: {other:?}"),
        };
        assert_eq!(0, result.outcome.stats().files());
        assert_eq!(0, result.outcome.stats().directories());
        assert_eq!(0, result.outcome.stats().bytes());
        assert_eq!(b"existing", std::fs::read(target.join("leaf")).unwrap().as_slice());
        assert_eq!(0o500, mode(&target));
        let snapshot = facts.snapshot();
        assert!(snapshot.modified_count >= 1);
        assert_eq!(0, snapshot.modified_bytes);
        assert_eq!(
            qubit_local_files::LocalPublicationState::NotPublished,
            snapshot.publication
        );

        let no_metadata_target = root.join("target-without-metadata");
        std::fs::create_dir(&no_metadata_target).unwrap();
        std::fs::write(no_metadata_target.join("leaf"), b"existing").unwrap();
        std::fs::set_permissions(&no_metadata_target, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (mut skipped, skipped_facts) = filesystem
            .submit_copy(
                &manager,
                Default::default(),
                LocalCopyRequest {
                    source: request_path(&source, "source"),
                    destination: request_path(&no_metadata_target, "target-without-metadata"),
                    options: LocalCopyOptions::new()
                        .with_tree_source()
                        .with_conflict(LocalCopyConflictPolicy::Skip)
                        .with_type_conflict(LocalCopyTypeConflictPolicy::Skip),
                },
                Default::default(),
            )
            .unwrap();
        assert!(matches!(
            skipped.wait_outcome().unwrap(),
            qubit_operation::OperationOutcome::Succeeded(_)
        ));
        let snapshot = skipped_facts.snapshot();
        assert_eq!(0, snapshot.modified_count);
        assert_eq!(
            qubit_local_files::LocalPublicationState::NotPublished,
            snapshot.publication
        );
        assert_eq!(0o700, mode(&no_metadata_target));
        std::fs::set_permissions(&no_metadata_target, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn test_async_copy_counts_confirmed_directory_permissions_with_mixed_and_partial_results() {
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path, value: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(value)).unwrap();
    }

    for rooted in [false, true] {
        for partial in [false, true] {
            let scratch = tempfile::tempdir().unwrap();
            let root = scratch.path().join("root");
            let source = root.join("source");
            let target = root.join("target");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::create_dir_all(&target).unwrap();
            let manager = OperationManager::new(ManagerOptions::default()).unwrap();
            let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
            let filesystem = if rooted {
                block_on(AsyncLocalFileSystem::rooted(executor, root.clone())).unwrap()
            } else {
                block_on(AsyncLocalFileSystem::host(executor)).unwrap()
            };
            let request_path = |path: &Path, relative: &str| {
                if rooted {
                    PathBuf::from(relative)
                } else {
                    path.to_path_buf()
                }
            };

            if partial {
                std::fs::create_dir(source.join("first")).unwrap();
                std::fs::create_dir(source.join("second")).unwrap();
                std::fs::create_dir(target.join("first")).unwrap();
                std::fs::create_dir(target.join("second")).unwrap();
                let names = std::fs::read_dir(&source)
                    .unwrap()
                    .map(|entry| entry.unwrap().file_name())
                    .collect::<Vec<_>>();
                let empty_name = names[0].clone();
                let deep_name = names[1].clone();
                std::fs::create_dir(source.join(&deep_name).join("deep")).unwrap();
                std::fs::create_dir(source.join(&deep_name).join("deep/too-deep")).unwrap();
                mode(&source.join(&empty_name), 0o500);
                mode(&target.join(&empty_name), 0o700);
                mode(&source.join(&deep_name), 0o500);
                mode(&target.join(&deep_name), 0o700);
                let (mut operation, facts) = filesystem
                    .submit_copy(
                        &manager,
                        Default::default(),
                        LocalCopyRequest {
                            source: request_path(&source, "source"),
                            destination: request_path(&target, "target"),
                            options: LocalCopyOptions::new()
                                .with_tree_source()
                                .with_conflict(LocalCopyConflictPolicy::Fail)
                                .with_max_depth(2)
                                .with_max_open_directories(4)
                                .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
                        },
                        Default::default(),
                    )
                    .unwrap();
                let outcome = operation.wait_outcome().unwrap();
                assert!(
                    matches!(outcome, qubit_operation::OperationOutcome::Failed(_)),
                    "{outcome:?}"
                );
                let snapshot = facts.snapshot();
                assert_eq!(2, snapshot.modified_count, "facts: {snapshot:?}");
                assert_eq!(0, snapshot.modified_bytes);
                assert_eq!(
                    qubit_local_files::LocalPublicationState::Published,
                    snapshot.publication
                );
                assert_eq!(
                    0o500,
                    std::fs::metadata(target.join(empty_name)).unwrap().permissions().mode() & 0o777
                );
                assert!(target.join(&deep_name).join("deep").is_dir());
                mode(&target.join(deep_name), 0o700);
            } else {
                std::fs::write(source.join("leaf"), b"payload").unwrap();
                mode(&source, 0o500);
                mode(&target, 0o700);
                let (mut operation, facts) = filesystem
                    .submit_copy(
                        &manager,
                        Default::default(),
                        LocalCopyRequest {
                            source: request_path(&source, "source"),
                            destination: request_path(&target, "target"),
                            options: LocalCopyOptions::new()
                                .with_tree_source()
                                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                                .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
                        },
                        Default::default(),
                    )
                    .unwrap();
                assert!(matches!(
                    operation.wait_outcome().unwrap(),
                    qubit_operation::OperationOutcome::Succeeded(_)
                ));
                let snapshot = facts.snapshot();
                assert_eq!(2, snapshot.modified_count);
                assert_eq!(
                    qubit_local_files::LocalPublicationState::Published,
                    snapshot.publication
                );
                mode(&target, 0o700);
            }
            mode(&source, 0o700);
        }
    }
}

#[test]
fn test_delete_reports_missing_file_and_directory_errors() {
    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);

    let (mut missing_file, missing_file_facts) = filesystem
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
        .unwrap();
    let missing_error = match missing_file.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => error,
        other => panic!("missing deletion must fail: {other:?}"),
    };
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::NotFound,
        missing_error.kind()
    );
    assert_eq!(None, missing_error.effect_state());
    let missing_snapshot = missing_file_facts.snapshot();
    assert_eq!(0, missing_snapshot.modified_count);
    assert_eq!(
        qubit_local_files::LocalPublicationState::Unknown,
        missing_snapshot.publication
    );
    assert!(missing_snapshot.range_unknown);

    let refused_tree = directory.path().join("refused-delete");
    std::fs::create_dir(&refused_tree).unwrap();
    std::fs::write(refused_tree.join("payload"), b"data").unwrap();
    let (mut refused_delete, refused_facts) = filesystem
        .submit_delete(
            &manager,
            Default::default(),
            LocalDeleteRequest {
                path: refused_tree.clone(),
                options: LocalDeleteOptions::new().with_recursive().with_max_entries(0),
                directory: true,
            },
            Default::default(),
        )
        .unwrap();
    let refused_error = match refused_delete.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => error,
        other => panic!("zero-entry budget must reject deletion: {other:?}"),
    };
    assert_eq!(
        Some(qubit_local_files::error::LocalFileEffectState::Unchanged),
        refused_error.effect_state()
    );
    let refused_snapshot = refused_facts.snapshot();
    assert_eq!(0, refused_snapshot.modified_count);
    assert_eq!(
        qubit_local_files::LocalPublicationState::NotPublished,
        refused_snapshot.publication
    );
    assert!(!refused_snapshot.range_unknown);
    assert_eq!(b"data", std::fs::read(refused_tree.join("payload")).unwrap().as_slice());

    let partial_tree = directory.path().join("partial-delete");
    std::fs::create_dir(&partial_tree).unwrap();
    for name in ["first", "second"] {
        std::fs::create_dir(partial_tree.join(name)).unwrap();
        std::fs::write(partial_tree.join(name).join("payload"), b"data").unwrap();
    }
    let (mut partial_delete, partial_facts) = filesystem
        .submit_delete(
            &manager,
            Default::default(),
            LocalDeleteRequest {
                path: partial_tree.clone(),
                options: LocalDeleteOptions::new().with_recursive().with_max_entries(4),
                directory: true,
            },
            Default::default(),
        )
        .unwrap();
    let partial_error = match partial_delete.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Failed(error) => error,
        other => panic!("budgeted recursive deletion must fail after a partial effect: {other:?}"),
    };
    assert_eq!(
        Some(qubit_local_files::error::LocalFileEffectState::PartiallyApplied),
        partial_error.effect_state()
    );
    let partial_snapshot = partial_facts.snapshot();
    assert_eq!(1, partial_snapshot.modified_count);
    assert_eq!(
        qubit_local_files::LocalPublicationState::Unknown,
        partial_snapshot.publication
    );
    assert!(partial_snapshot.range_unknown);
    assert!(partial_tree.exists());
    assert_eq!(1, std::fs::read_dir(&partial_tree).unwrap().count());

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
