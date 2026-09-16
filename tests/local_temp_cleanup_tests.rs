// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Retained temporary-directory cleanup budgets and source authority.

use std::fs;
use std::io::ErrorKind;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::MAIN_SEPARATOR_STR;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalTempDirectory;
#[cfg(feature = "test-support")]
use qubit_local_files::error::LocalFileEffectState;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalTempCleanupLimits;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalTempSourceState;
#[cfg(feature = "test-support")]
use qubit_local_files::test_support::install_test_fault;
use tempfile::TempDir;
use tempfile::tempdir;

/// Creates a resource and its independent parent for either namespace.
fn fixture(rooted: bool) -> (TempDir, LocalTempDirectory, PathBuf) {
    let parent = tempdir().expect("scratch parent should exist");
    let filesystem = if rooted {
        LocalFileSystem::rooted(parent.path())
    } else {
        LocalFileSystem::host()
    }
    .expect("filesystem should open");
    let options = LocalTempDirectoryOptions::new().with_parent(if rooted { Path::new("") } else { parent.path() });
    let directory = filesystem
        .create_temp_directory_with_options(&options)
        .expect("temporary directory should exist");
    let native = if rooted {
        parent.path().join(
            directory
                .path()
                .strip_prefix(MAIN_SEPARATOR_STR)
                .expect("virtual root prefix"),
        )
    } else {
        directory.path().to_path_buf()
    };
    (parent, directory, native)
}

/// Explicit exhaustion and the subsequent Drop use the same retained limit.
#[test]
fn test_temp_cleanup_budget_preserved_for_drop() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        for index in 0..32 {
            fs::write(native.join(format!("sentinel-{index}")), b"data").expect("leaf should exist");
        }
        directory.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(1));
        let error = directory
            .cleanup()
            .expect_err("enumeration must exhaust the budget before removing children");
        assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
        assert_eq!(LocalTempSourceState::Owned, directory.source_state());
        drop(directory);
        assert!(native.exists());
        assert_eq!(
            32,
            fs::read_dir(&native)
                .expect("bounded Drop must preserve the tree")
                .count()
        );
    }
}

/// Limits can be independently removed and creation carries them to the guard.
#[test]
fn test_temp_cleanup_limits_options_and_creation() {
    let limits = LocalTempCleanupLimits::new()
        .with_max_depth(1)
        .with_max_entries(2)
        .with_max_pending_path_bytes(3)
        .with_deadline(Duration::from_secs(4));
    assert_eq!(Some(1), limits.max_depth());
    assert_eq!(Some(2), limits.max_entries());
    assert_eq!(Some(3), limits.max_pending_path_bytes());
    assert_eq!(Some(Duration::from_secs(4)), limits.deadline());
    assert_eq!(
        LocalTempCleanupLimits::default(),
        limits
            .without_max_depth()
            .without_max_entries()
            .without_max_pending_path_bytes()
            .without_deadline()
    );
    for rooted in [false, true] {
        let parent = tempdir().expect("scratch parent");
        let filesystem = if rooted {
            LocalFileSystem::rooted(parent.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let options = LocalTempDirectoryOptions::new()
            .with_parent(if rooted { Path::new("") } else { parent.path() })
            .with_cleanup_limits(limits);
        assert_eq!(limits, options.cleanup_limits());
        let mut directory = filesystem
            .create_temp_directory_with_options(&options)
            .expect("resource");
        assert_eq!(limits, directory.cleanup_limits());
        directory.set_cleanup_limits(LocalTempCleanupLimits::new());
        directory.cleanup().expect("unbounded cleanup");
    }
}

/// Root depth is zero; entries include the root but exclude the sandbox.
#[test]
fn test_temp_cleanup_exact_entry_depth_and_pending_boundaries() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        directory.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(0).with_max_entries(1));
        directory
            .cleanup()
            .expect("one empty root needs one entry even with sandbox");
        assert!(!native.parent().expect("sandbox").exists());

        let (_parent, mut directory, native) = fixture(rooted);
        fs::write(native.join("child"), b"data").expect("leaf");
        let charged = if rooted {
            directory.path().strip_prefix(MAIN_SEPARATOR_STR).expect("virtual root")
        } else {
            directory.path()
        };
        let bytes = charged.as_os_str().len() + charged.join("child").as_os_str().len();
        directory.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_pending_path_bytes(bytes - 1));
        let error = directory.cleanup().expect_err("one byte short must fail");
        assert_eq!(
            LocalResourceKind::PendingPathBytes,
            error.resource_limit_error().expect("budget facts").resource()
        );
        assert!(native.join("child").exists());
        directory.set_cleanup_limits(
            LocalTempCleanupLimits::new()
                .with_max_pending_path_bytes(bytes)
                .with_max_depth(1)
                .with_max_entries(2),
        );
        directory
            .cleanup()
            .expect("exact fresh budgets should remove root, child and sandbox");
    }
}

/// Zero limits and an expired deadline fail before any source deletion.
#[test]
fn test_temp_cleanup_rejects_limits_before_mutation() {
    for rooted in [false, true] {
        for limits in [
            LocalTempCleanupLimits::new().with_max_entries(0),
            LocalTempCleanupLimits::new().with_max_depth(0),
            LocalTempCleanupLimits::new().with_max_pending_path_bytes(0),
            LocalTempCleanupLimits::new().with_deadline(Duration::ZERO),
        ] {
            let (_parent, mut directory, native) = fixture(rooted);
            fs::write(native.join("child"), b"data").expect("leaf");
            directory.set_cleanup_limits(limits);
            let error = directory.cleanup().expect_err("budget must reject tree");
            if limits.deadline().is_some() {
                assert_eq!(ErrorKind::TimedOut, error.io_error_kind());
            } else {
                assert!(error.resource_limit_error().is_some());
            }
            assert!(native.join("child").exists());
            directory.set_cleanup_limits(LocalTempCleanupLimits::new());
            directory.cleanup().expect("lifting budget permits retry");
        }
    }
}

/// A second native failure preserves both partial effects and source ownership.
#[cfg(feature = "test-support")]
#[test]
fn test_temp_cleanup_partial_failure_retains_owned_source() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        for name in ["first", "second"] {
            fs::write(native.join(name), b"data").expect("leaf should exist");
        }
        let fault = install_test_fault("temp-directory-remove-second").expect("fault should install");
        let error = directory
            .cleanup()
            .expect_err("second removal must fail after the first leaf");
        assert_eq!(LocalFileErrorKind::PublicationIncomplete, error.kind());
        assert_eq!(Some(LocalFileEffectState::PartiallyApplied), error.effect_state());
        assert_eq!(LocalTempSourceState::Owned, directory.source_state());
        assert_eq!(1, fs::read_dir(&native).expect("root should remain").count());
        assert_ne!(Some(directory.path()), error.path());
        drop(fault);
        directory.cleanup().expect("retry should remove remaining entries");
        assert!(!native.exists());
    }
}

/// Once the tree is removed, cleanup only retries its private sandbox.
#[cfg(feature = "test-support")]
#[test]
fn test_temp_cleanup_sandbox_failure_retains_cleanup_required() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        let fault = install_test_fault("temp-directory-sandbox-remove").expect("fault should install");
        let error = directory.cleanup().expect_err("sandbox removal must fail");
        assert_eq!(Some(LocalFileEffectState::PartiallyApplied), error.effect_state());
        assert_eq!(LocalTempSourceState::CleanupRequired, directory.source_state());
        assert!(!native.exists());
        drop(fault);
        fs::create_dir(&native).expect("external source replacement");
        fs::write(native.join("replacement"), b"safe").expect("replacement leaf");
        let retry_error = directory.cleanup().expect_err("replacement makes sandbox nonempty");
        assert_eq!(ErrorKind::DirectoryNotEmpty, retry_error.io_error_kind());
        assert_eq!(None, retry_error.effect_state());
        assert_eq!(LocalTempSourceState::CleanupRequired, directory.source_state());
        assert!(native.join("replacement").exists());
        fs::remove_dir_all(&native).expect("test removes its own replacement");
        directory
            .cleanup()
            .expect("sandbox-only retry should succeed without inspecting missing source");
        assert_eq!(LocalTempSourceState::Released, directory.source_state());
        directory.cleanup().expect("successful cleanup is idempotent");
    }
}

/// A missing identity locks both resource wrappers instead of retaining
/// authority.
#[test]
fn test_temp_identity_missing_source_locks_file_and_directory() {
    for rooted in [false, true] {
        let (parent, directory, native) = fixture(rooted);
        let public_parent = if rooted {
            Path::new(MAIN_SEPARATOR_STR)
        } else {
            parent.path()
        };
        fs::rename(&native, parent.path().join("saved-directory")).expect("move original away");
        let mut error = directory
            .persist(public_parent.join("target-directory"))
            .expect_err("source is missing");
        assert_eq!(ErrorKind::NotFound, error.error().io_error_kind());
        assert_eq!(LocalPersistFailureState::NotPublished, error.state());
        assert_eq!(LocalTempSourceState::Indeterminate, error.source_state());
        assert!(error.resource_mut().cleanup().is_err());
        drop(error);
        assert!(parent.path().join("saved-directory").exists());

        let filesystem = if rooted {
            LocalFileSystem::rooted(parent.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let file = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(public_parent))
            .expect("temporary file");
        let native = if rooted {
            parent
                .path()
                .join(file.path().strip_prefix(MAIN_SEPARATOR_STR).expect("virtual root"))
        } else {
            file.path().to_path_buf()
        };
        fs::rename(&native, parent.path().join("saved-file")).expect("move original file away");
        let mut error = file
            .persist(public_parent.join("target-file"))
            .expect_err("source file is missing");
        assert_eq!(ErrorKind::NotFound, error.error().io_error_kind());
        assert_eq!(LocalPersistFailureState::NotPublished, error.state());
        assert_eq!(LocalTempSourceState::Indeterminate, error.source_state());
        assert!(error.resource_mut().write_all(b"must not write").is_err());
        assert!(error.resource_mut().cleanup().is_err());
        drop(error);
        assert_eq!(
            b"",
            fs::read(parent.path().join("saved-file"))
                .expect("original must remain")
                .as_slice()
        );
    }
}

/// Lazy traversal handles both depth and width with fresh exact entry budgets.
#[test]
fn test_temp_cleanup_deep_and_wide_trees() {
    for rooted in [false, true] {
        for deep in [false, true] {
            let (_parent, mut directory, native) = fixture(rooted);
            let mut current = native.clone();
            let levels = if deep { 32 } else { 1 };
            let leaves = if deep { 4 } else { 128 };
            for level in 0..levels {
                current = current.join(format!("level-{level}"));
                fs::create_dir(&current).expect("branch");
                for leaf in 0..leaves {
                    fs::write(current.join(format!("leaf-{leaf}")), b"data").expect("leaf");
                }
            }
            directory.set_cleanup_limits(
                LocalTempCleanupLimits::new()
                    .with_max_depth(levels + 1)
                    .with_max_entries(1 + levels * (1 + leaves)),
            );
            directory
                .cleanup()
                .expect("exact entry limit should handle deep and wide trees");
            assert!(!native.exists());
        }
    }
}

/// Replacing the root never authorizes removal of the replacement tree.
#[test]
fn test_temp_cleanup_replaced_root_preserves_both_trees() {
    for rooted in [false, true] {
        let (parent, mut directory, native) = fixture(rooted);
        fs::write(native.join("original"), b"original").expect("original leaf");
        let moved = parent.path().join("original-tree");
        fs::rename(&native, &moved).expect("move original");
        fs::create_dir(&native).expect("replacement directory");
        fs::write(native.join("replacement"), b"replacement").expect("replacement leaf");
        let error = directory.cleanup().expect_err("identity mismatch must fail");
        assert_eq!(ErrorKind::InvalidInput, error.io_error_kind());
        assert_eq!(LocalTempSourceState::Indeterminate, directory.source_state());
        drop(directory);
        assert!(native.join("replacement").exists());
        assert!(moved.join("original").exists());
    }
}

/// Directory symlinks are unlinked without touching an external target.
#[cfg(unix)]
#[test]
fn test_temp_cleanup_symlink_target_is_untouched() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        let outside = tempdir().expect("outside parent");
        fs::write(outside.path().join("sentinel"), b"safe").expect("outside leaf");
        symlink(outside.path(), native.join("link")).expect("directory link");
        directory.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_depth(1).with_max_entries(2));
        directory.cleanup().expect("link counts as one leaf");
        assert_eq!(
            b"safe",
            fs::read(outside.path().join("sentinel"))
                .expect("outside target")
                .as_slice()
        );
    }
}

/// A concurrent new child returns DirectoryNotEmpty without an unbounded
/// rescan.
#[cfg(feature = "test-support")]
#[test]
fn test_temp_cleanup_concurrent_child_preserves_owned_state() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        let fault = install_test_fault("temp-directory-concurrent-child").expect("fault");
        let error = directory
            .cleanup()
            .expect_err("concurrent child must stop final root removal");
        assert_eq!(ErrorKind::DirectoryNotEmpty, error.io_error_kind());
        assert_eq!(LocalTempSourceState::Owned, directory.source_state());
        assert!(native.join("late-child").exists());
        drop(fault);
        directory.cleanup().expect("fresh attempt should remove the new child");
    }
}

/// A disappeared queued child is an error and leaves root authority owned.
#[cfg(feature = "test-support")]
#[test]
fn test_temp_cleanup_missing_child_retains_owned_source() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        fs::write(native.join("child"), b"data").expect("leaf");
        let fault = install_test_fault("temp-directory-child-not-found").expect("fault");
        let error = directory.cleanup().expect_err("queued child disappeared");
        assert_eq!(ErrorKind::NotFound, error.io_error_kind());
        assert_eq!(LocalTempSourceState::Owned, directory.source_state());
        assert_eq!(Some(directory.path().join("child").as_path()), error.path());
        drop(fault);
        directory.cleanup().expect("fresh retry");
    }
}

/// The sandbox shares the original deadline boundary; the next call is fresh.
#[cfg(feature = "test-support")]
#[test]
fn test_temp_cleanup_deadline_covers_sandbox_and_restarts_on_retry() {
    for rooted in [false, true] {
        let (_parent, mut directory, native) = fixture(rooted);
        directory.set_cleanup_limits(LocalTempCleanupLimits::new().with_deadline(Duration::from_secs(60)));
        let fault = install_test_fault("local-delete-deadline-5").expect("fault");
        let error = directory
            .cleanup()
            .expect_err("deadline expires before sandbox removal");
        assert_eq!(ErrorKind::TimedOut, error.io_error_kind());
        assert_eq!(Some(LocalFileEffectState::PartiallyApplied), error.effect_state());
        assert_eq!(LocalTempSourceState::CleanupRequired, directory.source_state());
        assert!(!native.exists());
        assert!(native.parent().expect("sandbox").exists());
        drop(fault);
        directory
            .cleanup()
            .expect("fresh same-limit attempt can release sandbox");
        assert_eq!(LocalTempSourceState::Released, directory.source_state());
    }
}

/// Rooted cleanup keeps using its opened authority after its diagnostic root
/// moves.
#[test]
fn test_temp_cleanup_rooted_authority_survives_root_rename() {
    let parent = tempdir().expect("scratch parent");
    let root = parent.path().join("root");
    fs::create_dir(&root).expect("root directory");
    let filesystem = LocalFileSystem::rooted(&root).expect("opened root");
    let mut directory = filesystem.create_temp_directory().expect("temporary directory");
    let relative = directory
        .path()
        .strip_prefix(MAIN_SEPARATOR_STR)
        .expect("virtual root")
        .to_path_buf();
    fs::write(root.join(&relative).join("child"), b"data").expect("source leaf");
    let moved = parent.path().join("moved-root");
    fs::rename(&root, &moved).expect("move root authority");
    fs::create_dir(&root).expect("new diagnostic root");
    fs::write(root.join("sentinel"), b"safe").expect("replacement root leaf");
    directory.cleanup().expect("cleanup must use opened root");
    assert!(!moved.join(relative).exists());
    assert_eq!(
        b"safe",
        fs::read(root.join("sentinel"))
            .expect("replacement root remains")
            .as_slice()
    );
}

/// A file observed by the scheduler must not turn into permission to remove
/// an externally substituted empty directory at the native boundary.
#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_temp_cleanup_observed_file_replaced_by_directory_is_preserved() {
    let (_parent, mut directory, native) = fixture(true);
    let child = native.join("child");
    fs::write(&child, b"original").expect("original leaf");
    let fault = install_test_fault("temp-observed-file-becomes-directory").expect("scoped type replacement fault");
    let error = directory
        .cleanup()
        .expect_err("unlink must reject the substituted directory");
    assert!(matches!(
        error.io_error_kind(),
        ErrorKind::IsADirectory | ErrorKind::PermissionDenied
    ));
    assert_eq!(Some(directory.path().join("child").as_path()), error.path());
    assert_eq!(LocalTempSourceState::Owned, directory.source_state());
    assert!(child.is_dir());
    drop(fault);
}

/// A directory observed earlier must not authorize unlinking a replacement
/// ordinary file during the post-order removal phase.
#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_temp_cleanup_observed_directory_replaced_by_file_is_preserved() {
    let (_parent, mut directory, native) = fixture(true);
    let child = native.join("child");
    fs::create_dir(&child).expect("original child directory");
    let fault = install_test_fault("temp-observed-directory-becomes-file").expect("scoped type replacement fault");
    let error = directory.cleanup().expect_err("rmdir must reject the substituted file");
    assert_eq!(ErrorKind::NotADirectory, error.io_error_kind());
    assert_eq!(Some(directory.path().join("child").as_path()), error.path());
    assert_eq!(LocalTempSourceState::Owned, directory.source_state());
    assert_eq!(
        b"replacement",
        fs::read(&child).expect("replacement file remains").as_slice()
    );
    drop(fault);
}

/// A directory replaced by a symbolic link is neither followed nor unlinked
/// by the directory removal operation, preserving its external target.
#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_temp_cleanup_observed_directory_replaced_by_symlink_is_preserved() {
    let (parent, mut directory, native) = fixture(true);
    let outside = parent.path().join("outside");
    fs::create_dir(&outside).expect("outside directory");
    fs::write(outside.join("sentinel"), b"safe").expect("outside sentinel");
    let child = native.join("child");
    fs::create_dir(&child).expect("original child directory");
    let fault = install_test_fault("temp-observed-directory-becomes-symlink").expect("scoped type replacement fault");
    let error = directory
        .cleanup()
        .expect_err("rmdir must reject the substituted symlink");
    assert_eq!(ErrorKind::NotADirectory, error.io_error_kind());
    assert_eq!(Some(directory.path().join("child").as_path()), error.path());
    assert_eq!(LocalTempSourceState::Owned, directory.source_state());
    assert!(
        fs::symlink_metadata(&child)
            .expect("replacement link remains")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::canonicalize(&outside).expect("outside target resolves"),
        fs::canonicalize(&child).expect("replacement link must point at the tested target")
    );
    assert_eq!(
        b"safe",
        fs::read(outside.join("sentinel")).expect("target remains").as_slice()
    );
    drop(fault);
}
