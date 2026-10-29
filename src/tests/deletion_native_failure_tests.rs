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

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::error::LocalFileEffectState;
use crate::options::LocalCopyOptions;
use crate::options::LocalCopyTypeConflictPolicy;
use crate::options::LocalDeleteOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::outcome::LocalCopyFailureState;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

fn assert_injected_cause(error: &dyn std::error::Error) {
    let mut current = Some(error);
    let mut found = false;
    while let Some(source) = current {
        if source.to_string() == "injected native I/O contract failure" {
            found = true;
            break;
        }
        current = source.source();
    }
    assert!(found, "native failure must survive context conversion: {error:?}");
}

fn assert_delete_error(
    error: &LocalFileError,
    effect: LocalFileEffectState,
    operation: LocalFileOperation,
    path: &Path,
) {
    assert_eq!(error.operation(), operation);
    assert_eq!(error.path(), Some(path));
    assert_eq!(error.effect_state(), Some(effect));
    assert_eq!(
        error.kind(),
        if effect == LocalFileEffectState::PartiallyApplied {
            LocalFileErrorKind::PublicationIncomplete
        } else {
            LocalFileErrorKind::Io
        }
    );
    assert_injected_cause(error);
}

fn deletion_cases() -> [(NativeIoPoint, usize, &'static str, LocalFileEffectState); 7] {
    [
        (NativeIoPoint::DeleteMetadata, 1, "", LocalFileEffectState::Unchanged),
        (
            NativeIoPoint::DeleteMetadata,
            3,
            "branch/leaf",
            LocalFileEffectState::Unchanged,
        ),
        (
            NativeIoPoint::DeleteReadDirectory,
            1,
            "",
            LocalFileEffectState::Unchanged,
        ),
        (
            NativeIoPoint::DeleteReadDirectory,
            2,
            "",
            LocalFileEffectState::Unchanged,
        ),
        (
            NativeIoPoint::DeleteRemoveFile,
            1,
            "branch/leaf",
            LocalFileEffectState::Indeterminate,
        ),
        (
            NativeIoPoint::DeleteRemoveDirectory,
            1,
            "branch",
            LocalFileEffectState::PartiallyApplied,
        ),
        (
            NativeIoPoint::DeleteRemoveDirectory,
            2,
            "",
            LocalFileEffectState::PartiallyApplied,
        ),
    ]
}

#[test]
fn test_recursive_deletion_native_failures_preserve_exact_effect_and_allow_retry() {
    for rooted in [false, true] {
        for (point, nth, suffix, effect) in deletion_cases() {
            let scratch = tempfile::tempdir().expect("fixture should exist");
            let native = fs::canonicalize(scratch.path()).expect("fixture root should resolve");
            fs::create_dir_all(native.join("tree/branch")).expect("tree should exist");
            fs::write(native.join("tree/branch/leaf"), b"original leaf").expect("leaf should exist");
            fs::write(native.join("user-data"), b"protected user data").expect("sentinel should exist");
            let filesystem = if rooted {
                LocalFileSystem::rooted(&native)
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let root = if rooted {
                Path::new("/tree").to_path_buf()
            } else {
                native.join("tree")
            };
            let options = LocalDeleteOptions::new().with_recursive();
            let guard = NativeIoFaultGuard::fail_on(point, nth);
            let error = filesystem
                .delete_directory_with_options(&root, &options)
                .expect_err("native deletion failure must propagate");
            assert!(
                guard.fired(),
                "{point:?} occurrence {nth} should fire for rooted={rooted}"
            );
            assert_delete_error(
                &error,
                effect,
                LocalFileOperation::DeleteDirectory,
                &if effect == LocalFileEffectState::PartiallyApplied {
                    root.join(suffix)
                } else {
                    root.clone()
                },
            );
            drop(guard);
            if effect == LocalFileEffectState::PartiallyApplied {
                assert!(
                    !native.join("tree/branch/leaf").exists(),
                    "successful child removals must remain visible"
                );
                assert_eq!(native.join("tree/branch").exists(), nth == 1);
            } else {
                assert_eq!(
                    fs::read(native.join("tree/branch/leaf"))
                        .expect("pre-removal faults should preserve all original contents"),
                    b"original leaf"
                );
            }
            assert!(native.join("tree").is_dir());
            assert!(
                filesystem
                    .delete_directory_with_options(&root, &options)
                    .expect("same tree should support retry")
                    .deleted()
            );
            assert!(!native.join("tree").exists());
            assert_eq!(
                fs::read(native.join("user-data")).expect("tree deletion must preserve siblings"),
                b"protected user data"
            );
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
fn test_pre_remove_directory_failure_keeps_partial_children_and_retries_safely() {
    for (rooted, fault) in [
        (false, "host-delete-directory-entry-second"),
        (true, "rooted-delete-directory-entry-second"),
    ] {
        let scratch = tempfile::tempdir().expect("fixture should exist");
        let native = fs::canonicalize(scratch.path()).expect("fixture root should resolve");
        fs::create_dir_all(native.join("tree/branch")).expect("tree should exist");
        fs::write(native.join("tree/branch/leaf"), b"original leaf").expect("leaf should exist");
        fs::write(native.join("user-data"), b"protected user data").expect("sentinel should exist");
        let filesystem = if rooted {
            LocalFileSystem::rooted(&native)
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem should open");
        let root = if rooted {
            Path::new("/tree").to_path_buf()
        } else {
            native.join("tree")
        };
        let guard = crate::test_support::install_test_fault(fault).expect("pre-remove fault should install");
        let error = filesystem
            .delete_directory_with_options(&root, &LocalDeleteOptions::new().with_recursive())
            .expect_err("second removal must fail before unlinking the empty directory");
        assert_eq!(error.kind(), LocalFileErrorKind::PublicationIncomplete);
        assert_eq!(error.operation(), LocalFileOperation::DeleteDirectory);
        assert_eq!(error.effect_state(), Some(LocalFileEffectState::PartiallyApplied));
        assert_eq!(error.path(), Some(root.join("branch").as_path()));
        assert_eq!(
            error
                .io_error()
                .expect("pre-remove failure must retain native cause")
                .raw_os_error(),
            Some(libc::EIO)
        );
        drop(guard);
        assert!(!native.join("tree/branch/leaf").exists());
        assert!(native.join("tree/branch").is_dir());
        assert!(
            filesystem
                .delete_directory_with_options(&root, &LocalDeleteOptions::new().with_recursive())
                .expect("partially removed tree should support retry")
                .deleted()
        );
        assert!(!native.join("tree").exists());
        assert_eq!(
            fs::read(native.join("user-data")).expect("retry must preserve unrelated siblings"),
            b"protected user data"
        );
    }
}

#[test]
fn test_temporary_directory_native_cleanup_failure_retains_resource_for_retry() {
    for rooted in [false, true] {
        for (point, nth, suffix, effect) in deletion_cases() {
            let scratch = tempfile::tempdir().expect("fixture should exist");
            let native = fs::canonicalize(scratch.path()).expect("fixture root should resolve");
            let filesystem = if rooted {
                LocalFileSystem::rooted(&native)
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let parent = if rooted { Path::new("/") } else { native.as_path() };
            let mut temporary = filesystem
                .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(parent))
                .expect("temporary resource should exist");
            let diagnostic = temporary.path().to_path_buf();
            let physical = if rooted {
                native.join(
                    diagnostic
                        .strip_prefix("/")
                        .expect("rooted path should be virtual absolute"),
                )
            } else {
                diagnostic.clone()
            };
            fs::create_dir(physical.join("branch")).expect("temporary child should exist");
            fs::write(physical.join("branch/leaf"), b"temporary leaf").expect("temporary leaf should exist");
            fs::write(native.join("user-data"), b"protected user data").expect("sentinel should exist");
            let guard = NativeIoFaultGuard::fail_on(point, nth);
            let error = temporary
                .cleanup()
                .expect_err("cleanup must expose native failures while retaining its owner");
            assert!(
                guard.fired(),
                "{point:?} occurrence {nth} should fire for rooted={rooted}"
            );
            assert_delete_error(&error, effect, LocalFileOperation::Cleanup, &diagnostic.join(suffix));
            drop(guard);
            assert!(
                physical.is_dir(),
                "failed cleanup should leave the owned directory recoverable"
            );
            if effect != LocalFileEffectState::PartiallyApplied {
                assert_eq!(
                    fs::read(physical.join("branch/leaf"))
                        .expect("pre-removal failure should preserve temporary contents"),
                    b"temporary leaf"
                );
            }
            temporary
                .cleanup()
                .expect("same temporary owner must recover without rebinding its identity");
            assert!(!physical.exists());
            assert_eq!(
                fs::read(native.join("user-data")).expect("temporary cleanup must preserve siblings"),
                b"protected user data"
            );
        }
    }
}

#[test]
fn test_rooted_type_replacement_cleanup_failure_preserves_source_and_aborts_staging() {
    for (point, nth, expected_state, leaf_removed) in [
        (
            NativeIoPoint::DeleteMetadata,
            1,
            LocalCopyFailureState::Unchanged,
            false,
        ),
        (
            NativeIoPoint::DeleteReadDirectory,
            1,
            LocalCopyFailureState::Unchanged,
            false,
        ),
        (
            NativeIoPoint::DeleteReadDirectory,
            2,
            LocalCopyFailureState::Unchanged,
            false,
        ),
        (
            NativeIoPoint::DeleteRemoveFile,
            1,
            LocalCopyFailureState::Indeterminate,
            false,
        ),
        (
            NativeIoPoint::DeleteRemoveDirectory,
            1,
            LocalCopyFailureState::PartiallyPublished,
            true,
        ),
    ] {
        let scratch = tempfile::tempdir().expect("fixture should exist");
        let native = fs::canonicalize(scratch.path()).expect("fixture root should resolve");
        fs::write(native.join("source"), b"new source bytes").expect("copy source should exist");
        fs::create_dir(native.join("target")).expect("conflicting target should exist");
        fs::write(native.join("target/old"), b"old target bytes").expect("old destination leaf should exist");
        fs::write(native.join("user-data"), b"protected user data").expect("sentinel should exist");
        let filesystem = LocalFileSystem::rooted(&native).expect("filesystem should open");
        let options = LocalCopyOptions::new().with_type_conflict(LocalCopyTypeConflictPolicy::Replace);
        let guard = NativeIoFaultGuard::fail_on(point, nth);
        let failure = filesystem
            .copy_with_options(Path::new("/source"), Path::new("/target"), &options)
            .expect_err("destination removal failure must prevent staged-file publication");
        assert!(guard.fired(), "{point:?} occurrence {nth} should fire");
        assert_eq!(failure.state(), expected_state);
        assert_eq!(failure.error().operation(), LocalFileOperation::Copy);
        assert_eq!(failure.request_source_path(), Some(Path::new("/source")));
        assert_eq!(failure.request_target_path(), Some(Path::new("/target")));
        assert_eq!(failure.failed_source_path(), Some(Path::new("/source")));
        assert_eq!(failure.failed_target_path(), Some(Path::new("/target")));
        assert_eq!(failure.partial_stats().files(), 0, "no replacement was published");
        assert!(
            failure.staging_path().is_none(),
            "successful abort should release its staging file"
        );
        assert!(failure.cleanup_error().is_none());
        assert_injected_cause(failure.error());
        drop(guard);
        assert_eq!(
            fs::read(native.join("source")).expect("cleanup failure must not mutate source"),
            b"new source bytes"
        );
        assert!(native.join("target").is_dir());
        assert_eq!(native.join("target/old").exists(), !leaf_removed);
        if !leaf_removed {
            assert_eq!(
                fs::read(native.join("target/old")).expect("old destination must remain before any removal"),
                b"old target bytes"
            );
        }
        assert_eq!(
            fs::read_dir(&native)
                .expect("aborted staging should leave no auxiliary entries")
                .count(),
            3
        );
        let outcome = filesystem
            .copy_with_options(Path::new("/source"), Path::new("/target"), &options)
            .expect("copy should retry successfully once native removal recovers");
        assert_eq!(outcome.stats().files(), 1);
        assert_eq!(
            fs::read(native.join("target")).expect("retry should publish source bytes"),
            b"new source bytes"
        );
        assert_eq!(
            fs::read(native.join("source")).expect("retry must preserve source"),
            b"new source bytes"
        );
        assert_eq!(
            fs::read(native.join("user-data")).expect("copy cleanup must preserve unrelated data"),
            b"protected user data"
        );
    }
}
