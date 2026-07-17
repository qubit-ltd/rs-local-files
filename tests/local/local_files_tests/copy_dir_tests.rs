// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use qubit_local_files::LocalCopyDirStage;
use qubit_local_files::{
    LocalCopyConflictPolicy,
    LocalCopyDirOptions,
    LocalCopyTypeConflictPolicy,
    LocalFiles,
};
use std::io::{
    Error,
    ErrorKind,
};

#[cfg(target_os = "linux")]
use super::super::test_support::SourceReadLease;
#[cfg(windows)]
use super::super::test_support::path_with_interior_nul;
#[cfg(target_os = "linux")]
use super::super::test_support::run_in_small_stack_process;
use super::super::test_support::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
    fs,
    temp_dir,
};
#[cfg(unix)]
use super::super::test_support::{
    PermissionsExt,
    short_temp_dir,
};

#[test]
fn test_copy_dir_all_with_copies_tree_and_reports_stats() {
    let dir = temp_dir("copy-dir");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir_all(src.join("nested")).unwrap();
    fs::write(src.join("a.txt"), b"abc").unwrap();
    fs::write(src.join("nested").join("b.txt"), b"12345").unwrap();

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect("directory tree should be copied");

    assert_eq!(2, stats.files);
    assert_eq!(2, stats.directories);
    assert_eq!(8, stats.bytes);
    assert_eq!(b"abc", fs::read(dst.join("a.txt")).unwrap().as_slice());
    assert_eq!(
        b"12345",
        fs::read(dst.join("nested").join("b.txt"))
            .unwrap()
            .as_slice()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_handles_deep_tree_on_small_stack() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::copy_dir_tests::",
        "test_copy_dir_all_with_handles_deep_tree_on_small_stack",
    );
    const CHILD_ENVIRONMENT: &str =
        "QUBIT_LOCAL_FILES_COPY_DIR_SMALL_STACK_CHILD";

    let Some(dir) =
        run_in_small_stack_process(TEST_NAME, CHILD_ENVIRONMENT, || {
            let dir = std::path::PathBuf::from(format!(
                "/tmp/qio-{}-deep-copy-dir",
                std::process::id(),
            ));
            drop(fs::remove_dir_all(&dir));
            let src = dir.join("src");
            let dst = dir.join("dst");
            fs::create_dir_all(&src)
                .expect("deep copy source should be created");
            let mut current = src.clone();
            let mut relative = std::path::PathBuf::new();
            for _ in 0..512 {
                current.push("d");
                relative.push("d");
                fs::create_dir(&current)
                    .expect("deep copy directory should be created");
            }
            fs::write(current.join("leaf"), b"x")
                .expect("deep copy leaf should be written");

            let stats = LocalFiles::copy_dir_all_with(
                &src,
                &dst,
                LocalCopyDirOptions::default(),
            )
            .expect("deep directory tree should be copied");

            assert_eq!(1, stats.files);
            assert_eq!(513, stats.directories);
            assert_eq!(1, stats.bytes);
            assert_eq!(
                b"x",
                fs::read(dst.join(relative).join("leaf"))
                    .expect("deep copied leaf should be readable")
                    .as_slice(),
            );
            dir
        })
    else {
        return;
    };

    fs::remove_dir_all(dir).expect("deep copy fixture should be removed");
}

#[test]
fn test_copy_dir_all_with_copies_into_existing_directory() {
    let dir = temp_dir("copy-dir-existing-dir");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&dst).unwrap();
    fs::write(src.join("data.txt"), b"data").unwrap();

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect("directory should be copied into existing directory");

    assert_eq!(1, stats.files);
    assert_eq!(0, stats.directories);
    assert_eq!(b"data", fs::read(dst.join("data.txt")).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_relative_missing_destination() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .expect("current dir lock should be acquired");
    let dir = temp_dir("copy-dir-relative");
    let src = dir.join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("data.txt"), b"data").unwrap();
    let _guard = CurrentDirGuard::change_to(&dir);

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        "relative-dst",
        LocalCopyDirOptions::default(),
    )
    .expect("relative destination should be copied");

    assert_eq!(1, stats.files);
    assert_eq!(
        b"data",
        fs::read(dir.join("relative-dst/data.txt"))
            .unwrap()
            .as_slice()
    );
    drop(_guard);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_rejects_invalid_source_and_nested_destination() {
    let dir = temp_dir("copy-dir-invalid");
    let src = dir.join("src");
    let src_file = dir.join("source-file.txt");
    fs::create_dir(&src).unwrap();
    fs::write(&src_file, b"file").unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src_file,
        dir.join("dst"),
        LocalCopyDirOptions::default(),
    )
    .expect_err("file source should be rejected");
    assert_eq!(ErrorKind::InvalidInput, error.kind());

    let error = LocalFiles::copy_dir_all_with(
        &src,
        src.join("nested").join("dst"),
        LocalCopyDirOptions::default(),
    )
    .expect_err("destination inside source should be rejected");
    assert_eq!(ErrorKind::InvalidInput, error.kind());

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_returns_destination_canonicalize_error() {
    let dir = temp_dir("copy-dir-dst-canonicalize-error");
    let src = dir.join("src");
    fs::create_dir(&src).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        std::path::Path::new(""),
        LocalCopyDirOptions::default(),
    )
    .expect_err("empty destination should fail canonicalization");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_rejects_existing_root_destination_without_overwrite()
{
    let dir = temp_dir("copy-dir-existing-root");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::write(&dst, b"not a directory").unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("existing root destination should be rejected");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_read_dir_error() {
    let dir = temp_dir("copy-dir-read-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("unreadable source directory should fail");

    fs::set_permissions(&src, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_nested_read_dir_error() {
    let dir = temp_dir("copy-dir-nested-read-error");
    let src = dir.join("src");
    let nested = src.join("nested");
    let dst = dir.join("dst");
    fs::create_dir_all(&nested).unwrap();
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("unreadable nested directory should fail");

    fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_source_entry_metadata_error_without_search_permission()
 {
    let dir = temp_dir("copy-dir-source-entry-metadata-error");
    let src = dir.join("src");
    let source_file = src.join("data.txt");
    let dst = dir.join("dst");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"data").expect("source file should be written");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o400))
        .expect("source search permission should be removed");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err(
        "source entries should not be inspectable without search permission",
    );

    fs::set_permissions(&src, fs::Permissions::from_mode(0o700))
        .expect("source permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::InspectSourceEntry, error.stage);
    assert_eq!(source_file, error.source_path);
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_destination_inspection_error_for_nul_path() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = temp_dir("copy-dir-nul-destination");
    let src = dir.join("src");
    fs::create_dir(&src).expect("source directory should be created");
    let dst = dir.join(OsString::from_vec(b"dst\0invalid".to_vec()));

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("destination NUL should fail native metadata inspection");

    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_validates_invalid_destination_before_missing_source()
{
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = temp_dir("copy-invalid-destination-first");
    let src = dir.join("missing-source");
    let dst = dir.join(OsString::from_vec(b"dst\0invalid".to_vec()));

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("invalid destination should fail before source inspection");

    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_rejects_existing_destination_without_overwrite() {
    let dir = temp_dir("copy-dir-existing");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&dst).unwrap();
    fs::write(src.join("data.txt"), b"new").unwrap();
    fs::write(dst.join("data.txt"), b"old").unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("existing destination file should be rejected");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(b"old", fs::read(dst.join("data.txt")).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_skips_existing_destination_files() {
    let dir = temp_dir("copy-dir-skip");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).expect("source directory should be created");
    fs::create_dir(&dst).expect("destination directory should be created");
    fs::write(src.join("data.txt"), b"new")
        .expect("source file should be written");
    fs::write(dst.join("data.txt"), b"old")
        .expect("destination fixture should be written");

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().with_conflict(LocalCopyConflictPolicy::Skip),
    )
    .expect("existing destination file should be skipped");

    assert_eq!(0, stats.files);
    assert_eq!(1, stats.skipped);
    assert_eq!(
        b"old",
        fs::read(dst.join("data.txt"))
            .expect("skipped destination should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Runs a recursive copy while a Linux file lease pauses its source open.
///
/// The copy worker cannot pass `File::open(source_file)` until `action` has
/// completed and the lease is released. Because the implementation creates
/// its staging file before opening the source, this provides exact
/// after-staging synchronization without filesystem polling or large files.
///
/// # Parameters
///
/// * `source_file` - Regular source file opened by the copy worker.
/// * `action` - Filesystem mutation performed after the source open blocks.
/// * `copy` - Recursive-copy operation executed by the worker.
///
/// # Returns
///
/// Value returned by `copy`.
///
/// # Panics
///
/// Panics when the lease cannot be acquired, the worker does not reach the
/// source open, the lease cannot be released, `action` panics, or the worker
/// panics. The lease is released and the worker is joined before an action
/// panic resumes.
#[cfg(target_os = "linux")]
fn run_copy_after_staging<T, A, C>(
    source_file: &std::path::Path,
    action: A,
    copy: C,
) -> T
where
    T: Send + 'static,
    A: FnOnce(),
    C: FnOnce() -> T + Send + 'static,
{
    let lease = SourceReadLease::acquire(source_file)
        .expect("source read lease should be acquired");
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let worker_start = start.clone();
    let worker = std::thread::spawn(move || {
        worker_start.wait();
        copy()
    });
    start.wait();
    if let Err(error) = lease.wait_for_break() {
        drop(lease.release());
        drop(worker.join());
        panic!(
            "copy worker should block while opening the leased source: {error}"
        );
    }
    let action_result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
    let release_result = lease.release();
    let worker_result = worker.join();
    if let Err(payload) = action_result {
        std::panic::resume_unwind(payload);
    }
    release_result.expect("source read lease should be released");
    worker_result.expect("copy worker should not panic")
}

/// Tests whether directory write restrictions are effective for this process.
///
/// Privileged Linux processes may bypass ordinary mode-bit checks. Tests that
/// rely on a cleanup `PermissionDenied` must skip in that environment.
#[cfg(target_os = "linux")]
pub(super) fn directory_write_restrictions_are_enforced(
    path: &std::path::Path,
) -> bool {
    let probe = path.join(".permission-probe");
    fs::set_permissions(path, fs::Permissions::from_mode(0o500))
        .expect("probe directory write permission should be removed");
    let create_result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe);
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("probe directory permissions should be restored");
    match create_result {
        Err(error) if error.kind() == ErrorKind::PermissionDenied => true,
        Ok(file) => {
            drop(file);
            fs::remove_file(probe).expect("permission probe should be removed");
            false
        }
        Err(error) => {
            panic!("permission probe should succeed or be denied: {error}")
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_reports_staging_cleanup_failure() {
    let dir = temp_dir("copy-dir-staging-cleanup-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::create_dir(&dst).expect("destination directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    if !directory_write_restrictions_are_enforced(&dst) {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }
    let restricted_dst = dst.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let error = run_copy_after_staging(
        &source_file,
        move || {
            fs::set_permissions(
                &restricted_dst,
                fs::Permissions::from_mode(0o500),
            )
            .expect("destination write permission should be removed");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::default(),
            )
        },
    )
    .expect_err("commit and staging cleanup should both fail");

    let temporary_path = error
        .temporary_path
        .clone()
        .expect("copy error should retain the staging path");
    let cleanup_error_kind = error
        .cleanup_error
        .as_ref()
        .map(Error::kind)
        .expect("copy error should retain the cleanup failure");
    let error_message = error.to_string();
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    let temporary_path_remained = temporary_path.exists();
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert_eq!(LocalCopyDirStage::CommitFile, error.stage);
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(ErrorKind::PermissionDenied, cleanup_error_kind);
    assert!(error_message.contains(&temporary_path.display().to_string()));
    assert!(error_message.contains("staging cleanup also failed"));
    assert!(
        temporary_path_remained,
        "failed cleanup should leave the reported staging path"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_reports_skipped_staging_cleanup_failure() {
    let dir = temp_dir("copy-dir-skipped-staging-cleanup-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let destination_file = dst.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::create_dir(&dst).expect("destination directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    if !directory_write_restrictions_are_enforced(&dst) {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }
    let restricted_dst = dst.clone();
    let raced_destination = destination_file.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let error = run_copy_after_staging(
        &source_file,
        move || {
            fs::write(&raced_destination, b"existing")
                .expect("racing destination should be written");
            fs::set_permissions(
                &restricted_dst,
                fs::Permissions::from_mode(0o500),
            )
            .expect("destination write permission should be removed");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Skip),
            )
        },
    )
    .expect_err("failed cleanup must make a skipped copy observable");

    let temporary_path = error
        .temporary_path
        .clone()
        .expect("cleanup error should retain the staging path");
    let error_message = error.to_string();
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    let temporary_path_remained = temporary_path.exists();
    let destination_contents = fs::read(&destination_file)
        .expect("racing destination should remain readable");
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert_eq!(LocalCopyDirStage::CleanupTemporaryFile, error.stage);
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert!(error.cleanup_error.is_none());
    assert!(error_message.contains(&temporary_path.display().to_string()));
    assert!(!error_message.contains("staging cleanup also failed"));
    assert!(temporary_path_remained);
    assert_eq!(b"existing", destination_contents.as_slice());
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_handles_destination_created_after_staging() {
    let dir = temp_dir("copy-dir-staging-conflict");
    let src = dir.join("src");
    let source_file = src.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");

    let skip_dst = dir.join("skip-dst");
    let skip_target = skip_dst.join("data.txt");
    fs::create_dir(&skip_dst)
        .expect("skip destination directory should be created");
    let copy_src = src.clone();
    let copy_dst = skip_dst.clone();
    let stats = run_copy_after_staging(
        &source_file,
        || {
            fs::write(&skip_target, b"raced")
                .expect("racing skip destination should be written");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Skip),
            )
        },
    )
    .expect("destination created after staging should be skipped");

    assert_eq!(0, stats.files);
    assert_eq!(1, stats.skipped);
    assert_eq!(
        b"raced",
        fs::read(&skip_target)
            .expect("racing skip destination should remain readable")
            .as_slice()
    );

    let fail_dst = dir.join("fail-dst");
    let fail_target = fail_dst.join("data.txt");
    fs::create_dir(&fail_dst)
        .expect("fail destination directory should be created");
    let copy_src = src.clone();
    let copy_dst = fail_dst.clone();
    let error = run_copy_after_staging(
        &source_file,
        || {
            fs::write(&fail_target, b"raced")
                .expect("racing fail destination should be written");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::default(),
            )
        },
    )
    .expect_err(
        "destination created after staging should fail conservative copy",
    );

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(LocalCopyDirStage::CommitFile, error.stage);
    assert_eq!(
        b"raced",
        fs::read(&fail_target)
            .expect("racing fail destination should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_keeps_conflicting_directory_until_source_is_staged() {
    let dir = temp_dir("copy-dir-stage-before-type-replace-exact");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let conflicting_dir = dst.join("data.txt");
    let marker = conflicting_dir.join("keep.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    fs::create_dir_all(&conflicting_dir)
        .expect("conflicting destination directory should be created");
    fs::write(&marker, b"keep").expect("destination marker should be written");
    let observed_marker = marker.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let stats = run_copy_after_staging(
        &source_file,
        || {
            assert_eq!(
                b"keep",
                fs::read(&observed_marker)
                    .expect("destination must remain before source read")
                    .as_slice()
            );
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Overwrite)
                    .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            )
        },
    )
    .expect("copy should replace the directory only after staging succeeds");

    assert_eq!(1, stats.files);
    assert_eq!(
        b"new",
        fs::read(&conflicting_dir)
            .expect("copied file should replace the conflicting directory")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_preserves_file_replacing_directory_after_staging() {
    let dir = temp_dir("copy-dir-directory-to-file-after-staging");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let destination_entry = dst.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    fs::create_dir_all(&destination_entry)
        .expect("conflicting destination directory should be created");
    let racing_entry = destination_entry.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let stats = run_copy_after_staging(
        &source_file,
        move || {
            fs::remove_dir(&racing_entry)
                .expect("conflicting destination directory should be removed");
            fs::write(&racing_entry, b"raced")
                .expect("racing destination file should be written");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Skip)
                    .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            )
        },
    )
    .expect("racing destination file should be preserved and skipped");

    assert_eq!(0, stats.files);
    assert_eq!(1, stats.skipped);
    assert_eq!(
        b"raced",
        fs::read(&destination_entry)
            .expect("racing destination file should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_rejects_file_replacing_directory_after_staging() {
    let dir = temp_dir("copy-dir-directory-to-file-fail-after-staging");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let destination_entry = dst.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    fs::create_dir_all(&destination_entry)
        .expect("conflicting destination directory should be created");
    let racing_entry = destination_entry.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let error = run_copy_after_staging(
        &source_file,
        move || {
            fs::remove_dir(&racing_entry)
                .expect("conflicting destination directory should be removed");
            fs::write(&racing_entry, b"raced")
                .expect("racing destination file should be written");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            )
        },
    )
    .expect_err("racing destination file should be rejected");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(LocalCopyDirStage::CommitFile, error.stage);
    assert_eq!(
        b"raced",
        fs::read(&destination_entry)
            .expect("racing destination file should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_commits_when_directory_disappears_after_staging() {
    let dir = temp_dir("copy-dir-directory-disappears-after-staging");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let destination_entry = dst.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    fs::create_dir_all(&destination_entry)
        .expect("conflicting destination directory should be created");
    let racing_entry = destination_entry.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let stats = run_copy_after_staging(
        &source_file,
        move || {
            fs::remove_dir(&racing_entry)
                .expect("conflicting destination directory should be removed");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Overwrite)
                    .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            )
        },
    )
    .expect("copy should commit after the destination directory disappears");

    assert_eq!(1, stats.files);
    assert_eq!(
        b"new",
        fs::read(&destination_entry)
            .expect("copied destination should be readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_reports_directory_removal_error_after_staging() {
    let dir = temp_dir("copy-dir-directory-removal-error-after-staging");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let destination_entry = dst.join("data.txt");
    let marker = destination_entry.join("keep.txt");
    let permission_probe = dst.join("permission-probe");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    fs::create_dir_all(&destination_entry)
        .expect("conflicting destination directory should be created");
    fs::write(&marker, b"keep").expect("destination marker should be written");
    fs::create_dir(&permission_probe)
        .expect("permission probe should be created");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o500))
        .expect("destination write permission should be removed");
    let probe_result = fs::remove_dir(&permission_probe);
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    match probe_result {
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {}
        Ok(()) => {
            fs::remove_dir_all(dir).expect("test directory should be removed");
            return;
        }
        Err(error) => panic!("permission probe should be removable: {error}"),
    }
    fs::remove_dir(&permission_probe)
        .expect("permission probe should be removed after restoring access");
    let restricted_dst = dst.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let result = run_copy_after_staging(
        &source_file,
        move || {
            fs::set_permissions(
                &restricted_dst,
                fs::Permissions::from_mode(0o500),
            )
            .expect("destination write permission should be removed");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Overwrite)
                    .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            )
        },
    );
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    let error = result
        .expect_err("non-writable destination should reject directory removal");

    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
    assert!(
        destination_entry.is_dir(),
        "failed removal must not commit the source file over the directory"
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_dir_all_with_reports_reinspection_error_after_staging() {
    let dir = temp_dir("copy-dir-reinspection-error-after-staging");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let destination_entry = dst.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    fs::create_dir_all(&destination_entry)
        .expect("conflicting destination directory should be created");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o400))
        .expect("destination search permission should be removed");
    if fs::symlink_metadata(&destination_entry).is_ok() {
        fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
            .expect("destination permissions should be restored");
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    let restricted_dst = dst.clone();
    let copy_src = src.clone();
    let copy_dst = dst.clone();

    let error = run_copy_after_staging(
        &source_file,
        move || {
            fs::set_permissions(
                &restricted_dst,
                fs::Permissions::from_mode(0o400),
            )
            .expect("destination search permission should be removed");
        },
        move || {
            LocalFiles::copy_dir_all_with(
                copy_src,
                copy_dst,
                LocalCopyDirOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Overwrite)
                    .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            )
        },
    )
    .expect_err("destination reinspection should report permission failure");

    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_destination_entry_inspection_error() {
    let dir = temp_dir("copy-dir-destination-inspection-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(src.join("data.txt"), b"data")
        .expect("source file should be written");
    fs::create_dir(&dst).expect("destination directory should be created");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o600))
        .expect("destination search permission should be removed");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("unsearchable destination should fail entry inspection");

    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_nested_destination_inspection_error() {
    let dir = temp_dir("copy-dir-nested-destination-inspection-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir_all(src.join("nested"))
        .expect("nested source directory should be created");
    fs::create_dir(&dst).expect("destination directory should be created");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o600))
        .expect("destination search permission should be removed");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("unsearchable destination should fail nested inspection");

    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_destination_removal_permission_error() {
    let dir = temp_dir("copy-dir-destination-removal-permission-error");
    let src = dir.join("src");
    let parent = dir.join("parent");
    let dst = parent.join("dst");
    fs::create_dir(&src).expect("source directory should be created");
    fs::create_dir(&parent).expect("destination parent should be created");
    fs::write(&dst, b"existing")
        .expect("conflicting destination file should be written");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500))
        .expect("destination parent write permission should be removed");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new()
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
    )
    .expect_err("non-writable parent should reject destination removal");

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
        .expect("destination parent permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_staging_file_creation_error() {
    let dir = temp_dir("copy-dir-staging-create-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(src.join("data.txt"), b"data")
        .expect("source file should be written");
    fs::create_dir(&dst).expect("destination directory should be created");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o500))
        .expect("destination write permission should be removed");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("non-writable destination should reject staging creation");

    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700))
        .expect("destination permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
}

#[test]
fn test_copy_dir_all_with_rejects_type_conflict_without_removing_directory() {
    let dir = temp_dir("copy-dir-type-conflict");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let conflicting_dir = dst.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::create_dir_all(&conflicting_dir)
        .expect("conflicting destination directory should be created");
    fs::write(src.join("data.txt"), b"new")
        .expect("source file should be written");
    fs::write(conflicting_dir.join("unrelated.txt"), b"keep")
        .expect("unrelated destination file should be written");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite),
    )
    .expect_err("type conflict should be rejected by default");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        b"keep",
        fs::read(conflicting_dir.join("unrelated.txt"))
            .expect("unrelated destination should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_copy_dir_all_with_replaces_existing_destination_directory_with_file() {
    let dir = temp_dir("copy-dir-replace-directory-with-file");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let destination_entry = dst.join("data.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(src.join("data.txt"), b"new")
        .expect("source file should be written");
    fs::create_dir_all(&destination_entry)
        .expect("conflicting destination directory should be created");
    fs::write(destination_entry.join("old.txt"), b"old")
        .expect("conflicting directory contents should be written");

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
    )
    .expect("destination directory should be replaced with the source file");

    assert_eq!(1, stats.files);
    assert_eq!(b"new", fs::read(&destination_entry).unwrap().as_slice());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_keeps_conflicting_directory_when_source_copy_fails() {
    let dir = temp_dir("copy-dir-stage-before-type-replace");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let source_file = src.join("data.txt");
    let conflicting_dir = dst.join("data.txt");
    let marker = conflicting_dir.join("keep.txt");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(&source_file, b"new").expect("source file should be written");
    fs::create_dir_all(&conflicting_dir)
        .expect("conflicting destination directory should be created");
    fs::write(&marker, b"keep").expect("destination marker should be written");
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o000))
        .expect("source permissions should be restricted");

    if fs::File::open(&source_file).is_ok() {
        fs::set_permissions(&source_file, fs::Permissions::from_mode(0o600))
            .unwrap();
        fs::remove_dir_all(dir).unwrap();
        return;
    }

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
    )
    .expect_err("unreadable source should fail before replacing destination");

    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o600))
        .unwrap();
    let marker_contents = fs::read(&marker);
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(
        b"keep",
        marker_contents
            .expect("conflicting destination must remain")
            .as_slice()
    );
}

#[test]
fn test_copy_dir_all_with_overwrites_existing_destinations() {
    let dir = temp_dir("copy-dir-overwrite");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("data.txt"), b"new").unwrap();
    fs::write(&dst, b"old file blocks destination directory").unwrap();

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
    )
    .expect("destination should be overwritten");

    assert_eq!(1, stats.files);
    assert_eq!(1, stats.directories);
    assert_eq!(b"new", fs::read(dst.join("data.txt")).unwrap().as_slice());

    fs::write(src.join("data.txt"), b"newer").unwrap();
    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
    )
    .expect("existing destination file should be overwritten");

    assert_eq!(1, stats.files);
    assert_eq!(0, stats.directories);
    assert_eq!(b"newer", fs::read(dst.join("data.txt")).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_preserves_root_file_when_type_replacement_removal_fails()
 {
    let dir = temp_dir("copy-dir-root-file-removal-error");
    let src = dir.join("src");
    let protected_parent = dir.join("protected");
    let dst = protected_parent.join("dst");
    fs::create_dir(&src).expect("source directory should be created");
    fs::create_dir(&protected_parent)
        .expect("destination parent should be created");
    fs::write(&dst, b"old").expect("destination file should be written");
    fs::set_permissions(&protected_parent, fs::Permissions::from_mode(0o500))
        .expect("destination parent write permission should be removed");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new()
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
    )
    .expect_err("unwritable parent should reject destination replacement");

    fs::set_permissions(&protected_parent, fs::Permissions::from_mode(0o700))
        .expect("destination parent permissions should be restored");
    let destination_contents =
        fs::read(&dst).expect("failed replacement should preserve destination");
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
    assert_eq!(b"old", destination_contents.as_slice());
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_symlink_options() {
    let dir = temp_dir("copy-dir-symlink");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let followed_dst = dir.join("followed-dst");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("target.txt"), b"target").unwrap();
    std::os::unix::fs::symlink(src.join("target.txt"), src.join("link.txt"))
        .unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("default copy should reject symlinks");
    assert_eq!(ErrorKind::Unsupported, error.kind());

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &followed_dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect("symlink target should be copied");

    assert_eq!(2, stats.files);
    assert_eq!(
        b"target",
        fs::read(followed_dst.join("link.txt")).unwrap().as_slice()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_follows_directory_symlink_entry() {
    let dir = temp_dir("copy-dir-symlink-entry-dir");
    let src = dir.join("src");
    let target = dir.join("target-dir");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("data.txt"), b"data").unwrap();
    std::os::unix::fs::symlink(&target, src.join("dir-link")).unwrap();

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect("directory symlink entry should be followed");

    assert_eq!(1, stats.files);
    assert_eq!(
        b"data",
        fs::read(dst.join("dir-link").join("data.txt"))
            .unwrap()
            .as_slice()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_rejects_directory_symlink_cycle_when_following() {
    let dir = temp_dir("copy-dir-symlink-cycle");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    std::os::unix::fs::symlink(&src, src.join("loop")).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect_err(
        "directory symlink cycles should be rejected before recursive copy",
    );

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_rejects_destination_inside_followed_directory_symlink_target()
 {
    let dir = temp_dir("copy-dir-symlink-target-contains-dst");
    let src = dir.join("src");
    let target = dir.join("target");
    let dst = target.join("dst");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, src.join("target-link")).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect_err(
        "destination inside followed symlink target should be rejected",
    );

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_directory_symlink_options() {
    let dir = temp_dir("copy-dir-symlink-dir");
    let target = dir.join("target");
    let src_link = dir.join("src-link");
    let dst = dir.join("dst");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("data.txt"), b"data").unwrap();
    std::os::unix::fs::symlink(&target, &src_link).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src_link,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("source symlink should be rejected by default");
    assert_eq!(ErrorKind::Unsupported, error.kind());

    let stats = LocalFiles::copy_dir_all_with(
        &src_link,
        &dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect("directory symlink should be followed");

    assert_eq!(1, stats.files);
    assert_eq!(b"data", fs::read(dst.join("data.txt")).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_rejects_unsupported_source_types() {
    use std::os::unix::net::UnixListener;

    let dir = short_temp_dir("unsupported");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    let socket = src.join("socket");
    let listener =
        UnixListener::bind(&socket).expect("unix socket should be created");

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("socket source should be rejected");

    assert_eq!(LocalCopyDirStage::InspectSourceEntry, error.stage);
    assert_eq!(socket, error.source_path);
    assert_eq!(dst.join("socket"), error.destination_path);
    assert_eq!(0, error.stats.files);
    assert_eq!(1, error.stats.directories);
    assert_eq!(0, error.stats.bytes);
    assert_eq!(0, error.stats.skipped);
    assert_eq!(ErrorKind::Unsupported, error.kind());
    drop(listener);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_rejects_unsupported_symlink_target_types() {
    use std::os::unix::net::UnixListener;

    let dir = short_temp_dir("unsupported-link");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    let socket = src.join("socket");
    let listener =
        UnixListener::bind(&socket).expect("unix socket should be created");
    std::os::unix::fs::symlink(&socket, src.join("socket-link")).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect_err("socket symlink target should be rejected");

    assert_eq!(ErrorKind::Unsupported, error.kind());
    drop(listener);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_does_not_preserve_permissions_by_default() {
    let dir = temp_dir("copy-dir-private-permissions");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).expect("source directory should be created");
    fs::write(src.join("data.txt"), b"data")
        .expect("source file should be written");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o755))
        .expect("source directory permissions should be set");
    fs::set_permissions(
        src.join("data.txt"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("source file permissions should be set");

    LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
        .expect("directory should be copied with private defaults");

    assert_eq!(
        0o700,
        fs::metadata(&dst)
            .expect("destination directory metadata should be readable")
            .permissions()
            .mode()
            & 0o777
    );
    assert_eq!(
        0o600,
        fs::metadata(dst.join("data.txt"))
            .expect("destination file metadata should be readable")
            .permissions()
            .mode()
            & 0o777
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_preserves_permissions() {
    let dir = temp_dir("copy-dir-permissions");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("data.txt"), b"data").unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o751)).unwrap();
    fs::set_permissions(
        src.join("data.txt"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();

    LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().preserve_permissions(),
    )
    .expect("permissions should be preserved");

    assert_eq!(
        0o751,
        fs::metadata(&dst).unwrap().permissions().mode() & 0o777
    );
    assert_eq!(
        0o640,
        fs::metadata(dst.join("data.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    );
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_preserves_read_only_directory_permissions() {
    let dir = temp_dir("copy-dir-read-only-permissions");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("data.txt"), b"data").unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o555)).unwrap();

    LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().preserve_permissions(),
    )
    .expect("read-only directory permissions should be preserved after copying children");

    assert_eq!(
        0o555,
        fs::metadata(&dst).unwrap().permissions().mode() & 0o777
    );
    assert_eq!(b"data", fs::read(dst.join("data.txt")).unwrap().as_slice());

    fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_file_copy_error() {
    let dir = temp_dir("copy-dir-file-copy-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let file = src.join("data.txt");
    fs::create_dir(&src).unwrap();
    fs::write(&file, b"data").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("unreadable source file should fail");

    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_returns_destination_create_error() {
    let dir = temp_dir("copy-destination-create-error");
    let src = dir.join("src");
    let dst = dir.join("missing-parent").join("dst");
    fs::create_dir(&src).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("missing destination parent should be reported");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_rejects_unsupported_directory_entry() {
    use std::os::unix::net::UnixListener;

    let dir = short_temp_dir("copy-unsupported-entry");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    let listener = UnixListener::bind(src.join("socket")).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("unsupported directory entry should be reported");

    assert_eq!(ErrorKind::Unsupported, error.kind());
    drop(listener);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_broken_symlink_entry_error_when_following() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("copy-broken-symlink-entry");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    symlink(src.join("missing"), src.join("broken-link")).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect_err("broken symlink target should be reported");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_returns_broken_root_symlink_error_when_following() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("copy-broken-root-symlink");
    let src = dir.join("src-link");
    let dst = dir.join("dst");
    symlink(dir.join("missing"), &src).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::new().follow_symlinks(),
    )
    .expect_err("broken root symlink target should be reported");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}
