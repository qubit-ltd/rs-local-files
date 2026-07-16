// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::{
    LocalAtomicWriteStage,
    LocalFiles,
};
use std::io::{
    Error,
    ErrorKind,
    Write,
};

#[cfg(unix)]
use super::super::test_support::PermissionsExt;
#[cfg(windows)]
use super::super::test_support::path_with_interior_nul;
use super::super::test_support::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
    count_atomic_temp_files,
    fs,
    temp_dir,
};
#[cfg(target_os = "linux")]
use super::copy_dir_tests::directory_write_restrictions_are_enforced;

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

    let result = LocalFiles::atomic_write_with(&path, |writer| {
        writer.write_all(b"durable")?;
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
    fs::write(&path, b"old").expect("original target should be written");

    let error = LocalFiles::atomic_write_with(&path, |writer| {
        writer.write_all(b"new")?;
        Err(Error::other("write failed"))
    })
    .expect_err("writer error should be returned");

    assert_eq!(LocalAtomicWriteStage::WriteTemporaryFile, error.stage);
    assert_eq!(path, error.path);
    assert!(error.temporary_path.is_some());
    assert!(!error.committed);
    assert_eq!(ErrorKind::Other, error.kind());
    assert_eq!("write failed", error.source.to_string());
    assert_eq!(
        b"old",
        fs::read(&path)
            .expect("original target should remain readable")
            .as_slice()
    );
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("atomic error fixture should be removed");
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
    let error = LocalFiles::atomic_write_with(&path, move |writer| {
        writer.write_all(b"new")?;
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
        let _ = LocalFiles::atomic_write_with(&path, |writer| {
            writer.write_all(b"new")?;
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

#[test]
fn test_atomic_write_with_uses_guarded_atomic_writer() {
    let dir = temp_dir("atomic-guarded-callback");
    let path = dir.join("out.txt");

    LocalFiles::atomic_write_with(
        &path,
        |writer: &mut qubit_local_files::LocalAtomicWriter| {
            writer.write_all(b"committed")
        },
    )
    .expect("guarded atomic callback should commit");

    assert_eq!(
        b"committed",
        fs::read(&path)
            .expect("committed destination should be readable")
            .as_slice(),
    );
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("atomic fixture should be removed");
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
fn test_atomic_write_does_not_inherit_symlink_target_permissions() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-symlink-permissions");
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    fs::write(&target, b"target").expect("target should be written");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o777))
        .expect("target permissions should be set");
    symlink(&target, &link).expect("symlink should be created");

    LocalFiles::atomic_write(&link, b"replacement")
        .expect("symlink path should be replaced");

    let replacement_mode =
        fs::metadata(&link).unwrap().permissions().mode() & 0o777;
    let target_mode =
        fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(0, replacement_mode & 0o177);
    assert_eq!(0o777, target_mode);
    assert_eq!(b"target", fs::read(&target).unwrap().as_slice());
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
fn test_atomic_write_replaces_self_referential_symlink() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-metadata-error");
    let path = dir.join("loop");
    symlink(&path, &path).unwrap();

    LocalFiles::atomic_write(&path, b"data")
        .expect("self-referential symlink should be replaced");

    assert!(
        !fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(b"data", fs::read(&path).unwrap().as_slice());
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
