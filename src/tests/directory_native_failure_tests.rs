// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::directory::LocalDirectoryHandle;
use crate::directory::LocalTreeValidationOptions;
use crate::observation::LocalSourceVerification;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::outcome::LocalTreeMutationState;
use crate::policy::LocalPrivacyRequirement;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

fn fixture() -> (tempfile::TempDir, PathBuf, LocalDirectoryHandle) {
    let scratch = tempfile::tempdir().expect("fixture should exist");
    let native = fs::canonicalize(scratch.path()).expect("fixture should have a native path");
    fs::create_dir_all(native.join("nested/deeper")).expect("nested fixture should exist");
    fs::write(native.join("nested/payload"), b"protected payload").expect("payload should exist");
    let parent = LocalFileSystem::rooted(&native)
        .expect("filesystem should open")
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("parent should open before faults are armed");
    (scratch, native, parent)
}

fn assert_native_failure(error: &LocalFileError, operation: LocalFileOperation, path: Option<&Path>) {
    assert_eq!(error.kind(), LocalFileErrorKind::Io, "{error:?}");
    assert_eq!(error.operation(), operation, "{error:?}");
    assert_eq!(error.path(), path, "{error:?}");
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    let mut found = false;
    while let Some(current) = source {
        if let Some(io) = current.downcast_ref::<std::io::Error>() {
            if io.kind() == std::io::ErrorKind::Other && io.to_string() == "injected native I/O contract failure" {
                found = true;
                break;
            }
        }
        source = current.source();
    }
    assert!(
        found,
        "the injected native cause must survive context conversion: {error:?}"
    );
}

#[test]
fn test_bound_child_retains_native_metadata_and_cursor_clone_failure_context() {
    let (_scratch, native, parent) = fixture();
    let options = LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new());
    for (point, nth) in [(NativeIoPoint::CursorMetadata, 2), (NativeIoPoint::CloneCursor, 1)] {
        let guard = NativeIoFaultGuard::fail_on(point, nth);
        let error = parent
            .open_directory(Path::new("nested"), &options)
            .expect_err("retaining an ancestor must propagate native observation failures");
        assert!(guard.fired(), "{point:?} occurrence {nth} should be reached");
        assert_native_failure(&error, LocalFileOperation::Metadata, Some(&native.join("nested")));
        drop(guard);
        let child = parent
            .open_directory(Path::new("nested"), &options)
            .expect("native authority should recover");
        let mut reader = child
            .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
            .expect("recovered child should retain usable authority");
        assert_eq!(
            reader.read_to_end_limited(64).expect("source should remain readable"),
            b"protected payload"
        );
        assert_eq!(
            reader
                .verify_path_binding()
                .expect("recovered child binding should verify"),
            LocalSourceVerification::Unchanged
        );
    }
}

#[test]
fn test_binding_deadline_preserves_retained_observations_and_reader_identity() {
    let (_scratch, native, _parent) = fixture();
    let filesystem = LocalFileSystem::rooted(&native).expect("filesystem should open");
    let options = LocalDirectoryOpenOptions::new()
        .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::from_secs(3600)));
    let parent = filesystem
        .open_directory_with_options(Path::new("/"), &options)
        .expect("positive opening budget should retain the root binding");
    let child = parent
        .open_directory(Path::new("nested"), &options)
        .expect("child binding should inherit ancestors within its time budget");
    let ancestors = child
        .ancestor_observations(&LocalListOptions::new().with_deadline(Duration::from_secs(3600)))
        .expect("bounded observation collection should finish and preserve identities");
    assert_eq!(ancestors.len(), 2);
    assert_eq!(
        ancestors[0].identity(),
        parent.observe().expect("root authority should remain live").identity()
    );
    assert_eq!(
        ancestors[1].identity(),
        child.observe().expect("child authority should remain live").identity()
    );
    let mut reader = child
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("reader should resolve under the retained binding deadline");
    assert_eq!(
        reader.read_to_end_limited(64).expect("read should preserve contents"),
        b"protected payload"
    );
    assert_eq!(
        reader
            .verify_path_binding()
            .expect("positive binding deadline should permit verification"),
        LocalSourceVerification::Unchanged
    );
}

#[test]
fn test_opening_parent_steps_preserves_original_recipe_on_native_validation_failure() {
    let (_scratch, native, _parent) = fixture();
    let filesystem = LocalFileSystem::rooted(&native).expect("filesystem should open before fault");
    let recipe = Path::new("nested/../nested/deeper");
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CursorMetadata, 1);
    let error = filesystem
        .open_directory_with_options(recipe, &LocalDirectoryOpenOptions::new())
        .expect_err("parent-step validation should preserve the caller's full opening recipe");
    assert!(guard.fired());
    assert_native_failure(&error, LocalFileOperation::OpenRoot, Some(recipe));
    drop(guard);
    let child = filesystem
        .open_directory_with_options(recipe, &LocalDirectoryOpenOptions::new())
        .expect("same opening recipe should recover after native failure");
    assert_eq!(
        child
            .observe()
            .expect("recovered directory should be observable")
            .identity(),
        filesystem
            .open_directory_with_options(Path::new("nested/deeper"), &LocalDirectoryOpenOptions::new())
            .expect("normalized path should identify the same authority")
            .observe()
            .expect("normalized authority should be observed")
            .identity()
    );
}

#[test]
fn test_entry_observation_second_descent_keeps_metadata_path_and_parent_authority() {
    let (_scratch, native, parent) = fixture();
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CursorDescend, 2);
    let error = parent
        .observe_entry(Path::new("nested/payload"))
        .expect_err("second traversal descent should fail independently of path resolution");
    assert!(guard.fired());
    assert_native_failure(
        &error,
        LocalFileOperation::Metadata,
        Some(&native.join("nested/payload")),
    );
    drop(guard);
    assert_eq!(
        parent
            .observe_entry(Path::new("nested/payload"))
            .expect("entry should remain observable")
            .metadata()
            .len(),
        17
    );
    assert_eq!(
        fs::read(native.join("nested/payload")).expect("failed observation cannot mutate data"),
        b"protected payload"
    );
}

#[test]
fn test_directory_binding_reopened_authority_clone_failure_preserves_reader() {
    let (_scratch, native, parent) = fixture();
    let child = parent
        .open_directory(
            Path::new("nested"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound child should open");
    let mut reader = child
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("bound reader should open before fault");
    // Reopening performs a best-effort limits probe before the binding clone.
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 3);
    let result = reader.verify_path_binding();
    assert!(guard.fired());
    let error = result.expect_err("clone after reopening the first directory anchor should fail");
    assert_native_failure(&error, LocalFileOperation::Metadata, Some(&native));
    drop(guard);
    assert_eq!(
        reader
            .verify_path_binding()
            .expect("same reader binding should recover"),
        LocalSourceVerification::Unchanged
    );
    assert_eq!(
        reader
            .read_to_end_limited(64)
            .expect("reader should retain its original file"),
        b"protected payload"
    );
}

#[test]
fn test_private_stage_reopen_late_authority_failure_does_not_remove_stage() {
    let (_scratch, native, parent) = fixture();
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect("private stage should exist before fault");
    let name = PathBuf::from(stage.relative_name());
    let identity = stage.identity().expect("private stage should have an identity").clone();
    let options = LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required);
    for (point, nth, path) in [
        (NativeIoPoint::CloneAuthority, 4, Some(native.join(&name))),
        (NativeIoPoint::Observation, 1, None),
    ] {
        let guard = NativeIoFaultGuard::fail_on(point, nth);
        let error = parent
            .open_staged_directory(&name, &identity, &options)
            .expect_err("reopen must propagate late native failures without cleanup");
        assert!(guard.fired());
        assert_native_failure(&error, LocalFileOperation::Metadata, path.as_deref());
        drop(guard);
        assert_eq!(
            parent.observe_entry(&name).expect("named stage must remain").identity(),
            Some(&identity)
        );
    }
    let recovered = parent
        .open_staged_directory(&name, &identity, &options)
        .expect("reopening should recover after fault removal");
    assert_eq!(recovered.identity(), Some(&identity));
    // The original owner remains responsible for the same object until cleanup.
    let _location = recovered.retain_in_place();
    stage
        .cleanup()
        .expect("original owner should retain safe cleanup authority");
    assert!(!native.join(&name).exists());
}

#[test]
fn test_private_stage_reopen_handle_metadata_failure_preserves_identity_and_contents() {
    use std::os::unix::fs::PermissionsExt;

    let (_scratch, native, parent) = fixture();
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect("private stage should exist before the metadata fault");
    let name = PathBuf::from(stage.relative_name());
    let path = native.join(&name);
    let identity = stage
        .identity()
        .expect("private stage should have a native identity")
        .clone();
    fs::write(path.join("retained-payload"), b"private stage bytes").expect("stage payload should exist");
    fs::set_permissions(path.join("retained-payload"), fs::Permissions::from_mode(0o600))
        .expect("payload should satisfy required stage privacy before injecting metadata failure");
    let options = LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required);
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::HandleMetadata, 1);
    let error = parent
        .open_staged_directory(&name, &identity, &options)
        .expect_err("private reopening must propagate same-handle metadata failure");
    assert!(
        guard.fired(),
        "private permission verification must query the retained handle"
    );
    assert_native_failure(&error, LocalFileOperation::Metadata, Some(&path));
    drop(guard);
    assert_eq!(
        parent
            .observe_entry(&name)
            .expect("failed reopen should retain the named stage")
            .identity(),
        Some(&identity)
    );
    assert_eq!(
        fs::read(path.join("retained-payload")).expect("failed reopen must preserve stage contents"),
        b"private stage bytes"
    );
    let recovered = parent
        .open_staged_directory(&name, &identity, &options)
        .expect("same stage should reopen after metadata querying recovers");
    assert_eq!(recovered.identity(), Some(&identity));
    let mut reader = recovered
        .directory()
        .open_observed_reader(Path::new("retained-payload"), &LocalObservedReadOptions::new())
        .expect("recovered authority should open the preserved payload");
    assert_eq!(
        reader
            .read_to_end_limited(64)
            .expect("recovered authority should read original bytes"),
        b"private stage bytes"
    );
    drop(reader);
    let _location = recovered.retain_in_place();
    stage
        .cleanup()
        .expect("original cleanup owner must remain usable after failed reopening");
    assert!(!path.exists());
    assert_eq!(
        fs::read(native.join("nested/payload")).expect("stage cleanup must preserve unrelated data"),
        b"protected payload"
    );
}

#[test]
fn test_private_stage_creation_late_native_failures_return_recoverable_identity_owners() {
    let (_scratch, native, parent) = fixture();
    for (point, nth, operation, relative_error) in [
        (NativeIoPoint::CloneAuthority, 2, LocalFileOperation::OpenRoot, true),
        (NativeIoPoint::CloneAuthority, 4, LocalFileOperation::Metadata, false),
        (NativeIoPoint::OpenCursor, 1, LocalFileOperation::OpenRoot, true),
        (NativeIoPoint::OpenCursor, 2, LocalFileOperation::OpenRoot, false),
        (NativeIoPoint::CursorMetadata, 2, LocalFileOperation::Metadata, false),
        (NativeIoPoint::CursorDescend, 1, LocalFileOperation::OpenRoot, false),
        (NativeIoPoint::Observation, 2, LocalFileOperation::Metadata, false),
    ] {
        let guard = NativeIoFaultGuard::fail_on(point, nth);
        let error = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
            .err()
            .unwrap_or_else(|| {
                panic!(
                    "{point:?} occurrence {nth} unexpectedly succeeded; fired={}",
                    guard.fired()
                )
            });
        assert!(guard.fired(), "{point:?} occurrence {nth} should be reached");
        assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
        assert_eq!(error.retained_resources().len(), 1);
        let owner = &error.retained_resources()[0];
        let path = owner.diagnostic_path().to_path_buf();
        assert_eq!(path.parent(), Some(native.as_path()));
        let identity = owner
            .identity()
            .expect("created authority must establish a cleanup identity")
            .clone();
        let expected_path = if point == NativeIoPoint::Observation {
            None
        } else if relative_error {
            Some(Path::new(path.file_name().expect("stage should have a basename")))
        } else {
            Some(path.as_path())
        };
        assert_native_failure(error.cause(), operation, expected_path);
        drop(guard);
        let name = Path::new(path.file_name().expect("stage should have a basename"));
        assert_eq!(
            parent
                .observe_entry(name)
                .expect("failed stage should remain named")
                .identity(),
            Some(&identity)
        );
        let (_cause, stage, resources) = error.into_parts();
        assert!(stage.is_none());
        for resource in resources {
            resource
                .cleanup()
                .expect("retained identity owner must clean up after recovery");
        }
        assert!(!path.exists());
        assert_eq!(
            fs::read(native.join("nested/payload")).expect("cleanup must preserve unrelated payload"),
            b"protected payload"
        );
    }
}

#[test]
fn test_private_stage_creation_observation_failure_keeps_unproven_stage_until_explicit_reopen() {
    let (_scratch, native, parent) = fixture();
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 1);
    let error = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect_err("first created-object observation should fail");
    assert!(guard.fired());
    assert_native_failure(error.cause(), LocalFileOperation::Metadata, None);
    assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
    assert_eq!(error.retained_resources().len(), 1);
    let path = error.retained_resources()[0].diagnostic_path().to_path_buf();
    assert!(error.retained_resources()[0].identity().is_none());
    drop(guard);
    let (_cause, stage, resources) = error.into_parts();
    assert!(stage.is_none());
    for resource in resources {
        let cleanup = resource
            .cleanup()
            .expect_err("unknown identity cannot authorize deletion");
        assert_eq!(cleanup.cause().kind(), LocalFileErrorKind::InvalidState);
        assert_eq!(cleanup.cause().operation(), LocalFileOperation::Cleanup);
        assert_eq!(cleanup.cause().path(), Some(path.as_path()));
        for owner in cleanup.into_retained_resources() {
            let _location = owner.retain_in_place();
        }
    }
    assert!(path.is_dir());
    let name = Path::new(path.file_name().expect("stage should have a basename"));
    let identity = parent
        .observe_entry(name)
        .expect("recovered observation should establish current identity")
        .identity()
        .expect("native identity should exist")
        .clone();
    let reopened = parent
        .open_staged_directory(
            name,
            &identity,
            &LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("explicit observation permits a validated reopen");
    reopened.cleanup().expect("verified stage should now support cleanup");
    assert!(!path.exists());
    assert_eq!(
        fs::read(native.join("nested/payload")).expect("cleanup must preserve unrelated payload"),
        b"protected payload"
    );
}
