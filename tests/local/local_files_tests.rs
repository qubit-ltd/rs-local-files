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
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalAtomicWriteStage,
    LocalCopyConflictPolicy,
    LocalCopyDirOptions,
    LocalCopyTypeConflictPolicy,
    LocalFiles,
};
use std::io::{
    Error,
    ErrorKind,
    Read,
    Write,
};

#[cfg(target_os = "linux")]
use super::test_support::SourceReadLease;
#[cfg(windows)]
use super::test_support::path_with_interior_nul;
use super::test_support::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
    count_atomic_temp_files,
    fs,
    temp_dir,
};
#[cfg(unix)]
use super::test_support::{
    PermissionsExt,
    short_temp_dir,
};

#[test]
fn test_atomic_write_creates_parent_directories_and_replaces_file() {
    let dir = temp_dir("atomic-replace");
    let path = dir.join("nested").join("out.txt");

    LocalFiles::atomic_write(&path, b"first")
        .expect("first atomic write should succeed");
    LocalFiles::atomic_write(&path, b"second")
        .expect("second atomic write should replace file");

    assert_eq!(b"second", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_syncs_parents_of_newly_created_directories() {
    let dir = temp_dir("atomic-sync-created-parent-chain");
    let first_created_parent = dir.join("first");
    let path = first_created_parent.join("second").join("out.txt");
    let mut permission_check_is_effective = false;

    let result = LocalFiles::atomic_write_with(&path, |file| {
        file.write_all(b"durable")?;
        fs::set_permissions(
            &first_created_parent,
            fs::Permissions::from_mode(0o111),
        )?;
        permission_check_is_effective = matches!(
            fs::File::open(&first_created_parent),
            Err(error) if error.kind() == ErrorKind::PermissionDenied
        );
        Ok(())
    });

    fs::set_permissions(
        &first_created_parent,
        fs::Permissions::from_mode(0o700),
    )
    .expect("created parent permissions should be restored");
    if !permission_check_is_effective {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }
    let error = result.expect_err(
        "syncing the parent of a newly created directory should be attempted",
    );

    assert_eq!(LocalAtomicWriteStage::SyncParentDirectory, error.stage);
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert!(error.committed);
    assert_eq!(
        b"durable",
        fs::read(&path)
            .expect("committed destination should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_handles_lexical_parent_aliases() {
    let dir = temp_dir("atomic-lexical-parent-aliases");
    let aliased_parent = dir.join("created").join("..");
    let destination = aliased_parent.join("out.txt");

    LocalFiles::atomic_write(&destination, b"aliased")
        .expect("directory alias should resolve after its prefix is created");

    assert!(dir.join("created").is_dir());
    assert_eq!(
        b"aliased",
        fs::read(dir.join("out.txt"))
            .expect("aliased destination should be readable")
            .as_slice()
    );

    let blocker = dir.join("blocker");
    fs::write(&blocker, b"not a directory")
        .expect("blocking file should be written");
    let error = LocalFiles::atomic_write(
        dir.join("missing").join("..").join("blocker/out.txt"),
        b"blocked",
    )
    .expect_err("aliased regular-file parent should be rejected");

    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage);
    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_rejects_dangling_symlink_parent() {
    let dir = temp_dir("atomic-dangling-parent-symlink");
    let dangling = dir.join("dangling");
    std::os::unix::fs::symlink(dir.join("missing-target"), &dangling)
        .expect("dangling parent symlink should be created");

    let error = LocalFiles::atomic_write(dangling.join("out.txt"), b"blocked")
        .expect_err("dangling parent symlink should not become a directory");

    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage);
    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert!(
        fs::symlink_metadata(&dangling)
            .expect("dangling symlink should remain")
            .file_type()
            .is_symlink()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_reports_parent_chain_creation_error() {
    let dir = temp_dir("atomic-parent-chain-creation-error");
    let restricted = dir.join("restricted");
    let probe = restricted.join("probe");
    let destination = restricted.join("missing/out.txt");
    fs::create_dir(&restricted)
        .expect("restricted directory should be created");
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o500))
        .expect("restricted directory permissions should be set");
    let probe_result = fs::create_dir(&probe);
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
        .expect("restricted directory permissions should be restored");
    match probe_result {
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {}
        Ok(()) => {
            fs::remove_dir_all(dir).expect("test directory should be removed");
            return;
        }
        Err(error) => panic!("permission probe should be creatable: {error}"),
    }
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o500))
        .expect("restricted directory permissions should be set");

    let result = LocalFiles::atomic_write(&destination, b"blocked");
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
        .expect("restricted directory permissions should be restored");
    let error = result.expect_err("non-writable parent should reject creation");

    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage);
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_atomic_write_rejects_windows_target_with_interior_nul() {
    let dir = temp_dir("atomic-write-windows-nul-target");
    let prefix = dir.join("existing-target");
    fs::write(&prefix, b"original").expect("prefix target should be written");
    let target = path_with_interior_nul(&dir, "existing-target");

    let error = LocalFiles::atomic_write(&target, b"replacement")
        .expect_err("Windows NUL target should be rejected before replacement");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(b"original", fs::read(&prefix).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_atomic_write_ignores_windows_parent_sync_sharing_violation() {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_LIST_DIRECTORY: u32 = 0x0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const ERROR_SHARING_VIOLATION: i32 = 32;

    let dir = temp_dir("atomic-parent-sync-sharing-violation");
    let parent = dir.join("locked-parent");
    fs::create_dir(&parent).unwrap();

    let locked_parent = match std::fs::OpenOptions::new()
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&parent)
    {
        Ok(file) => file,
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
            fs::remove_dir_all(dir).unwrap();
            return;
        }
        Err(error) => panic!(
            "parent directory should be locked for restricted sharing: {error}"
        ),
    };

    let path = parent.join("out.txt");
    LocalFiles::atomic_write(&path, b"data").expect(
        "atomic write should ignore unavailable Windows parent directory sync",
    );
    assert_eq!(b"data", fs::read(&path).unwrap().as_slice());

    drop(locked_parent);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_preserves_existing_file_permissions() {
    let dir = temp_dir("atomic-permissions");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o754)).unwrap();

    LocalFiles::atomic_write(&path, b"new")
        .expect("atomic write should preserve permissions");

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(0o754, mode);
    assert_eq!(b"new", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_write_supports_parentless_relative_path() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .expect("current dir lock should be acquired");
    let dir = temp_dir("atomic-parentless");
    let _guard = CurrentDirGuard::change_to(&dir);

    LocalFiles::atomic_write("out.txt", b"data")
        .expect("parentless atomic write should succeed");

    assert_eq!(b"data", fs::read(dir.join("out.txt")).unwrap().as_slice());
    drop(_guard);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_write_creates_missing_relative_parent_chain() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = temp_dir("atomic-missing-relative-parent-chain");
    let _guard = CurrentDirGuard::change_to(&dir);

    LocalFiles::atomic_write("first/second/out.txt", b"relative")
        .expect("relative parent chain should be created");

    assert_eq!(
        b"relative",
        fs::read("first/second/out.txt")
            .expect("relative destination should be readable")
            .as_slice()
    );
    drop(_guard);
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_parent_inspection_error() {
    let dir = temp_dir("atomic-parent-inspection-error");
    let restricted = dir.join("restricted");
    let path = restricted.join("missing").join("out.txt");
    fs::create_dir(&restricted)
        .expect("restricted directory should be created");
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o000))
        .expect("restricted directory permissions should be set");
    let probe = fs::metadata(restricted.join("missing"));
    if !matches!(
        probe,
        Err(ref error) if error.kind() == ErrorKind::PermissionDenied
    ) {
        fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
            .expect("restricted directory permissions should be restored");
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }

    let error = LocalFiles::atomic_write(&path, b"blocked")
        .expect_err("unsearchable parent should reject atomic preparation");

    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
        .expect("restricted directory permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage);
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert!(!error.committed);
}

#[test]
fn test_atomic_write_with_preserves_existing_file_and_removes_temp_on_error() {
    let dir = temp_dir("atomic-error");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").unwrap();

    let error = LocalFiles::atomic_write_with(&path, |file| {
        file.write_all(b"new")?;
        Err(Error::other("write failed"))
    })
    .expect_err("writer error should be returned");

    assert_eq!(LocalAtomicWriteStage::WriteTemporaryFile, error.stage);
    assert_eq!(path, error.path);
    assert!(error.temporary_path.is_some());
    assert!(!error.committed);
    assert_eq!(ErrorKind::Other, error.kind());
    assert_eq!("write failed", error.source.to_string());
    assert_eq!(b"old", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_atomic_write_with_reports_temporary_cleanup_failure() {
    let dir = temp_dir("atomic-staging-cleanup-error");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("original target should be written");
    if !directory_write_restrictions_are_enforced(&dir) {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }

    let restricted_dir = dir.clone();
    let error = LocalFiles::atomic_write_with(&path, move |file| {
        file.write_all(b"new")?;
        fs::set_permissions(
            &restricted_dir,
            fs::Permissions::from_mode(0o500),
        )?;
        Err(Error::other("write failed"))
    })
    .expect_err("write and staging cleanup should both fail");

    let temporary_path = error
        .temporary_path
        .clone()
        .expect("atomic error should retain the staging path");
    let cleanup_error_kind = error
        .cleanup_error
        .as_ref()
        .map(Error::kind)
        .expect("atomic error should retain the cleanup failure");
    let error_message = error.to_string();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .expect("directory permissions should be restored");
    let temporary_path_remained = temporary_path.exists();
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert_eq!(LocalAtomicWriteStage::WriteTemporaryFile, error.stage);
    assert_eq!(ErrorKind::Other, error.kind());
    assert_eq!(ErrorKind::PermissionDenied, cleanup_error_kind);
    assert!(error_message.contains(&temporary_path.display().to_string()));
    assert!(error_message.contains("staging cleanup also failed"));
    assert!(temporary_path_remained);
}

#[test]
fn test_atomic_write_with_removes_temporary_file_when_callback_panics() {
    let dir = temp_dir("atomic-write-panic");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("original target should be written");

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = LocalFiles::atomic_write_with(&path, |file| {
            file.write_all(b"new")?;
            panic!("intentional atomic-write callback panic");
        });
    }));

    let contents = fs::read(&path).expect("destination should remain readable");
    let temporary_file_count = count_atomic_temp_files(&dir);
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert!(panic.is_err());
    assert_eq!(b"old", contents.as_slice());
    assert_eq!(0, temporary_file_count, "staging file must be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_atomic_write_with_returns_temporary_sync_error() {
    let dir = temp_dir("atomic-sync-error");
    let path = dir.join("out.txt");

    let error = LocalFiles::atomic_write_with(&path, |file| {
        *file = fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("/dev/full should be writable");
        Ok(())
    })
    .expect_err("syncing /dev/full should fail");

    assert_eq!(LocalAtomicWriteStage::SyncTemporaryFile, error.stage);
    assert!(!error.committed);
    assert!(!path.exists());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_atomic_write_with_returns_permission_preservation_error() {
    let dir = temp_dir("atomic-permission-error");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").unwrap();

    let error = LocalFiles::atomic_write_with(&path, |file| {
        *file = fs::File::open("/proc/self/status")
            .expect("process status should be readable");
        Ok(())
    })
    .expect_err("changing process status permissions should fail");

    assert_eq!(LocalAtomicWriteStage::PreservePermissions, error.stage);
    assert!(!error.committed);
    assert_eq!(b"original", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_reader_and_writer_replace_old_buffered_helpers() {
    let dir = temp_dir("buffered");
    let path = dir.join("a").join("b").join("data.txt");

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
                .with_parent(),
        )
        .expect("writer should be created");
        writer.write_all(b"abc").unwrap();
        writer.close().unwrap();
    }

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::default().buffered(),
        )
        .expect("buffered writer should be created");
        writer.write_all(b"xyz").unwrap();
        writer.close().unwrap();
    }

    let mut reader =
        LocalFiles::open_reader(&path, FileReadOptions::buffered())
            .expect("reader should open");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    assert_eq!(b"xyz", content.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_reader_returns_open_error() {
    let dir = temp_dir("open-error");

    let error = LocalFiles::open_reader(
        dir.join("missing.txt"),
        FileReadOptions::default(),
    )
    .expect_err("missing file should return open error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    let source = std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<Error>())
        .expect(
            "path context should retain the native I/O error as its source",
        );
    assert_eq!(ErrorKind::NotFound, source.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_open_reader_returns_open_error_after_metadata_success() {
    let dir = temp_dir("open-reader-permission-error");
    let path = dir.join("data.txt");
    fs::write(&path, b"payload").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

    let error = LocalFiles::open_reader(&path, FileReadOptions::default())
        .expect_err("unreadable file should return open error");

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_respects_modes_parent_creation_and_buffering_options() {
    let dir = temp_dir("open-writer-options");
    let path = dir.join("nested").join("data.txt");

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::CreateNew)
                .with_parent()
                .buffered(),
        )
        .expect("create-new writer should create missing parents");
        assert!(writer.is_buffered());
        writer.write_all(b"one").unwrap();
        writer.close().unwrap();
    }

    let error = LocalFiles::open_writer(
        &path,
        FileWriteOptions::new(FileWriteMode::CreateNew),
    )
    .expect_err("create-new mode should reject existing files");
    assert_eq!(ErrorKind::AlreadyExists, error.kind());

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::AppendExisting),
        )
        .expect("append-existing writer should open existing files");
        writer.write_all(b"-two").unwrap();
        writer.close().unwrap();
    }
    assert_eq!(b"one-two", fs::read(&path).unwrap().as_slice());

    {
        let mut writer =
            LocalFiles::open_writer(&path, FileWriteOptions::default())
                .expect("default writer should create or truncate");
        writer.write_all(b"three").unwrap();
        writer.close().unwrap();
    }
    assert_eq!(b"three", fs::read(&path).unwrap().as_slice());

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_reader_and_writer_cover_unbuffered_and_append_or_create_modes() {
    let dir = temp_dir("open-writer-extra-modes");
    let path = dir.join("data.txt");
    fs::write(&path, b"abcdef").unwrap();

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::OpenExistingAtStart),
        )
        .expect("open-existing-at-start writer should open");
        assert!(!writer.is_buffered());
        writer.write_all(b"XY").unwrap();
        writer.close().unwrap();
    }
    assert_eq!(b"XYcdef", fs::read(&path).unwrap().as_slice());

    {
        let mut writer = LocalFiles::open_writer(
            &path,
            FileWriteOptions::new(FileWriteMode::AppendOrCreate)
                .buffered_with_capacity(16)
                .expect("positive buffer capacity should be accepted"),
        )
        .expect("append-or-create writer should open");
        assert!(writer.is_buffered());
        writer.write_all(b"-tail").unwrap();
        writer.close().unwrap();
    }

    let mut reader =
        LocalFiles::open_reader(&path, FileReadOptions::unbuffered())
            .expect("unbuffered reader should open");
    assert!(!reader.is_buffered());
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    assert_eq!(b"XYcdef-tail", content.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_returns_open_error_for_missing_parent_without_parent_creation()
 {
    let dir = temp_dir("open-writer-missing-parent");

    let error = LocalFiles::open_writer(
        dir.join("missing").join("data.txt"),
        FileWriteOptions::default(),
    )
    .expect_err("missing parent should return writer open error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_exists_metadata_and_list_report_local_paths() {
    let dir = temp_dir("metadata-list");
    let path = dir.join("data.txt");
    fs::write(&path, b"abc").unwrap();

    let mut names = LocalFiles::list(&dir)
        .expect("directory should be listed")
        .map(|entry| entry.expect("entry should be readable").file_name())
        .collect::<Vec<_>>();
    names.sort();

    assert!(
        LocalFiles::exists(&path).expect("existing file should be checked")
    );
    assert_eq!(3, LocalFiles::metadata(&path).unwrap().len());
    assert_eq!(vec![std::ffi::OsString::from("data.txt")], names);
    assert!(!LocalFiles::exists(dir.join("missing.txt")).unwrap());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_writer_returns_parent_error() {
    let dir = temp_dir("parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = LocalFiles::open_writer(
        file_parent.join("child.txt"),
        FileWriteOptions::default().with_parent(),
    )
    .expect_err("file parent should return create-dir error");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_ensure_dir_and_ensure_parent_create_missing_directories() {
    let dir = temp_dir("ensure");
    let child_dir = dir.join("a").join("b");
    let child_file = dir.join("c").join("d").join("out.txt");

    LocalFiles::ensure_dir(&child_dir).expect("directory should be created");
    LocalFiles::ensure_parent(&child_file).expect("parent should be created");

    assert!(child_dir.is_dir());
    assert!(child_file.parent().unwrap().is_dir());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_sums_regular_files_and_ignores_symlinks() {
    let dir = temp_dir("dir-size");
    fs::create_dir(dir.join("nested")).unwrap();
    fs::write(dir.join("a.txt"), b"abc").unwrap();
    fs::write(dir.join("nested").join("b.txt"), b"12345").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("a.txt"), dir.join("link.txt"))
        .unwrap();

    let size =
        LocalFiles::dir_size(&dir).expect("directory size should be computed");

    assert_eq!(8, size);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_rejects_non_directory() {
    let dir = temp_dir("dir-size-error");
    let path = dir.join("file.txt");
    fs::write(&path, b"data").unwrap();

    let error = LocalFiles::dir_size(&path)
        .expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_returns_missing_path_error() {
    let dir = temp_dir("dir-size-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::dir_size(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_dir_size_returns_read_dir_error() {
    let dir = temp_dir("dir-size-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::dir_size(&dir)
        .expect_err("unreadable directory should fail");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_removes_children_and_keeps_directory() {
    let dir = temp_dir("clean-dir");
    fs::create_dir(dir.join("nested")).unwrap();
    fs::write(dir.join("nested").join("child.txt"), b"child").unwrap();
    fs::write(dir.join("file.txt"), b"file").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("file.txt"), dir.join("link.txt"))
        .unwrap();

    LocalFiles::clean_dir(&dir).expect("directory should be cleaned");

    assert!(dir.is_dir());
    assert_eq!(0, fs::read_dir(&dir).unwrap().count());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_rejects_non_directory() {
    let dir = temp_dir("clean-dir-error");
    let path = dir.join("file.txt");
    fs::write(&path, b"data").unwrap();

    let error = LocalFiles::clean_dir(&path)
        .expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_returns_missing_path_error() {
    let dir = temp_dir("clean-dir-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::clean_dir(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_clean_dir_returns_read_dir_error() {
    let dir = temp_dir("clean-dir-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::clean_dir(&dir)
        .expect_err("unreadable directory should fail");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_remove_any_removes_files_directories_and_symlinks() {
    let dir = temp_dir("remove-any");
    let file = dir.join("file.txt");
    let nested = dir.join("nested");
    fs::write(&file, b"file").unwrap();
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("child.txt"), b"child").unwrap();

    LocalFiles::remove_any(&file).expect("file should be removed");
    LocalFiles::remove_any(&nested).expect("directory should be removed");

    assert!(!file.exists());
    assert!(!nested.exists());

    #[cfg(unix)]
    {
        let target = dir.join("target.txt");
        let link = dir.join("link.txt");
        fs::write(&target, b"target").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        LocalFiles::remove_any(&link).expect("symlink should be removed");

        assert!(target.exists());
        assert!(!link.exists());
    }

    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn test_remove_any_removes_directory_symlink_without_removing_target() {
    use std::os::windows::fs::symlink_dir;

    let dir = temp_dir("remove-directory-symlink");
    let target = dir.join("target");
    let link = dir.join("link");
    fs::create_dir_all(&target).expect("target directory should be created");
    if let Err(error) = symlink_dir(&target, &link) {
        assert_eq!(ErrorKind::PermissionDenied, error.kind());
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }

    LocalFiles::remove_any(&link).expect("directory symlink should be removed");

    assert!(!link.exists());
    assert!(target.is_dir());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_remove_any_returns_missing_path_error() {
    let dir = temp_dir("remove-any-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::remove_any(&missing)
        .expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

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
fn directory_write_restrictions_are_enforced(path: &std::path::Path) -> bool {
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
fn test_atomic_write_replaces_symlink_itself_without_modifying_target() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-replace-symlink");
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    fs::write(&target, b"target").unwrap();
    symlink(&target, &link).unwrap();

    LocalFiles::atomic_write(&link, b"replacement")
        .expect("symlink path should be replaced");

    assert!(
        !fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(b"replacement", fs::read(&link).unwrap().as_slice());
    assert_eq!(b"target", fs::read(&target).unwrap().as_slice());
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

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_temp_create_error() {
    let dir = temp_dir("atomic-temp-create-error");
    let path = dir.join("out.txt");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();

    let error = LocalFiles::atomic_write(&path, b"data")
        .expect_err("unwritable dir should fail temp creation");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalAtomicWriteStage::CreateTemporaryFile, error.stage);
    assert!(!error.committed);
    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_metadata_error() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-metadata-error");
    let path = dir.join("loop");
    symlink(&path, &path).unwrap();

    let error = LocalFiles::atomic_write(&path, b"data")
        .expect_err("symlink loop metadata should fail");

    assert!(
        error
            .to_string()
            .contains("failed to read destination metadata")
    );
    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage);
    assert!(!error.committed);
    fs::remove_file(&path).unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_write_removes_temp_when_rename_fails() {
    let dir = temp_dir("rename-error");
    let path = dir.join("target-dir");
    fs::create_dir(&path).unwrap();

    let error = LocalFiles::atomic_write(&path, b"data")
        .expect_err("renaming over a directory should fail");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists
            | ErrorKind::IsADirectory
            | ErrorKind::Other
            | ErrorKind::PermissionDenied
    ));
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage);
    assert!(!error.committed);
    assert!(path.is_dir());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_parent_sync_open_error_when_directory_is_not_readable()
 {
    let dir = temp_dir("atomic-parent-sync-open-error");
    let parent = dir.join("parent");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o300)).unwrap();

    let result = LocalFiles::atomic_write(parent.join("out.txt"), b"data");

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    if let Err(error) = result {
        assert_eq!(ErrorKind::PermissionDenied, error.kind());
        assert_eq!(LocalAtomicWriteStage::SyncParentDirectory, error.stage);
        assert!(error.committed);
        assert_eq!(
            b"data",
            fs::read(parent.join("out.txt")).unwrap().as_slice()
        );
    }
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
fn test_dir_size_ignores_unsupported_directory_entries() {
    use std::os::unix::net::UnixListener;

    let dir = short_temp_dir("dir-size-socket-entry");
    fs::write(dir.join("data.bin"), b"abc").unwrap();
    let listener = UnixListener::bind(dir.join("socket")).unwrap();

    assert_eq!(3, LocalFiles::dir_size(&dir).unwrap());

    drop(listener);
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
