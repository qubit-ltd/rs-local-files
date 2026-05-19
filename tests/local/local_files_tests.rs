/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

pub(super) use std::fs;
pub(super) use std::io::{
    Error,
    ErrorKind,
    Read,
    Write,
};
#[cfg(unix)]
pub(super) use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};
use std::sync::{
    Mutex,
    Once,
};

pub(super) use qubit_local_fs::{
    LocalCopyDirOptions,
    LocalFiles,
    LocalTempDir,
    LocalTempFile,
};

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(super) static CURRENT_DIR_LOCK: Mutex<()> = Mutex::new(());
static LOGGER_INIT: Once = Once::new();

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, _record: &log::Record<'_>) {}

    fn flush(&self) {}
}

static TEST_LOGGER: TestLogger = TestLogger;

pub(super) fn ensure_test_logger() {
    LOGGER_INIT.call_once(|| {
        if log::set_logger(&TEST_LOGGER).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    });
}

pub(super) fn temp_dir(name: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "qubit-local-fs-local-tests-{}-{name}-{id}",
        std::process::id()
    ));
    drop(fs::remove_dir_all(&path));
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

#[cfg(unix)]
pub(super) fn short_temp_dir(name: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = PathBuf::from(format!("/tmp/qio-{}-{name}-{id}", std::process::id()));
    drop(fs::remove_dir_all(&path));
    fs::create_dir_all(&path).expect("short temp dir should be created");
    path
}

pub(super) fn count_atomic_temp_files(dir: &std::path::Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".atomic-write-")
        })
        .count()
}

pub(super) struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    pub(super) fn change_to(path: &std::path::Path) -> Self {
        let original = std::env::current_dir().expect("current dir should be readable");
        std::env::set_current_dir(path).expect("current dir should be changed");
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        drop(std::env::set_current_dir(&self.original));
    }
}

#[test]
fn test_atomic_write_creates_parent_directories_and_replaces_file() {
    let dir = temp_dir("atomic-replace");
    let path = dir.join("nested").join("out.txt");

    LocalFiles::atomic_write(&path, b"first").expect("first atomic write should succeed");
    LocalFiles::atomic_write(&path, b"second").expect("second atomic write should replace file");

    assert_eq!(b"second", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
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
        Err(error) => panic!("parent directory should be locked for restricted sharing: {error}"),
    };

    let path = parent.join("out.txt");
    LocalFiles::atomic_write(&path, b"data")
        .expect("atomic write should ignore unavailable Windows parent directory sync");
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

    LocalFiles::atomic_write(&path, b"new").expect("atomic write should preserve permissions");

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

    LocalFiles::atomic_write("out.txt", b"data").expect("parentless atomic write should succeed");

    assert_eq!(b"data", fs::read(dir.join("out.txt")).unwrap().as_slice());
    drop(_guard);
    fs::remove_dir_all(dir).unwrap();
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

    assert_eq!(ErrorKind::Other, error.kind());
    assert_eq!("write failed", error.to_string());
    assert_eq!(b"old", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_create_file_with_parent_and_buffered_helpers() {
    let dir = temp_dir("buffered");
    let path = dir.join("a").join("b").join("data.txt");

    {
        let mut file = LocalFiles::create_file_with_parent(&path).expect("file should be created");
        file.write_all(b"abc").unwrap();
    }

    {
        let mut writer = LocalFiles::create_buffered_writer_with_parent(&path)
            .expect("buffered writer should be created");
        writer.write_all(b"xyz").unwrap();
        writer.flush().unwrap();
    }

    let mut reader = LocalFiles::open_buffered_reader(&path).expect("buffered reader should open");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();

    assert_eq!(b"xyz", content.as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_open_buffered_reader_returns_open_error() {
    let dir = temp_dir("open-error");

    let error = LocalFiles::open_buffered_reader(dir.join("missing.txt"))
        .expect_err("missing file should return open error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_create_file_with_parent_returns_parent_error() {
    let dir = temp_dir("parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = LocalFiles::create_file_with_parent(file_parent.join("child.txt"))
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
    std::os::unix::fs::symlink(dir.join("a.txt"), dir.join("link.txt")).unwrap();

    let size = LocalFiles::dir_size(&dir).expect("directory size should be computed");

    assert_eq!(8, size);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_rejects_non_directory() {
    let dir = temp_dir("dir-size-error");
    let path = dir.join("file.txt");
    fs::write(&path, b"data").unwrap();

    let error = LocalFiles::dir_size(&path).expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_dir_size_returns_missing_path_error() {
    let dir = temp_dir("dir-size-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::dir_size(&missing).expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_dir_size_returns_read_dir_error() {
    let dir = temp_dir("dir-size-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::dir_size(&dir).expect_err("unreadable directory should fail");

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
    std::os::unix::fs::symlink(dir.join("file.txt"), dir.join("link.txt")).unwrap();

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

    let error = LocalFiles::clean_dir(&path).expect_err("file should not be accepted as directory");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_clean_dir_returns_missing_path_error() {
    let dir = temp_dir("clean-dir-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::clean_dir(&missing).expect_err("missing path should return an error");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_clean_dir_returns_read_dir_error() {
    let dir = temp_dir("clean-dir-read-error");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();

    let error = LocalFiles::clean_dir(&dir).expect_err("unreadable directory should fail");

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

#[test]
fn test_remove_any_returns_missing_path_error() {
    let dir = temp_dir("remove-any-missing");
    let missing = dir.join("missing");

    let error = LocalFiles::remove_any(&missing).expect_err("missing path should return an error");

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

    let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
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

    let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
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

    let stats = LocalFiles::copy_dir_all_with(&src, "relative-dst", LocalCopyDirOptions::default())
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

    let error =
        LocalFiles::copy_dir_all_with(&src_file, dir.join("dst"), LocalCopyDirOptions::default())
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
fn test_copy_dir_all_with_returns_missing_source_error() {
    let dir = temp_dir("copy-dir-missing-source");
    let missing = dir.join("missing");

    let error =
        LocalFiles::copy_dir_all_with(&missing, dir.join("dst"), LocalCopyDirOptions::default())
            .expect_err("missing source should return metadata error");

    assert_eq!(ErrorKind::NotFound, error.kind());
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
fn test_copy_dir_all_with_rejects_existing_root_destination_without_overwrite() {
    let dir = temp_dir("copy-dir-existing-root");
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir(&src).unwrap();
    fs::write(&dst, b"not a directory").unwrap();

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
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

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
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

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
        .expect_err("unreadable nested directory should fail");

    fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
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

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
        .expect_err("existing destination file should be rejected");

    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert_eq!(b"old", fs::read(dst.join("data.txt")).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
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
        LocalCopyDirOptions {
            overwrite: true,
            ..LocalCopyDirOptions::default()
        },
    )
    .expect("destination should be overwritten");

    assert_eq!(1, stats.files);
    assert_eq!(1, stats.directories);
    assert_eq!(b"new", fs::read(dst.join("data.txt")).unwrap().as_slice());

    fs::write(src.join("data.txt"), b"newer").unwrap();
    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions {
            overwrite: true,
            ..LocalCopyDirOptions::default()
        },
    )
    .expect("existing destination file should be overwritten");

    assert_eq!(1, stats.files);
    assert_eq!(0, stats.directories);
    assert_eq!(b"newer", fs::read(dst.join("data.txt")).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
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
    std::os::unix::fs::symlink(src.join("target.txt"), src.join("link.txt")).unwrap();

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
        .expect_err("default copy should reject symlinks");
    assert_eq!(ErrorKind::Unsupported, error.kind());

    let stats = LocalFiles::copy_dir_all_with(
        &src,
        &followed_dst,
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
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
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
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
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
    )
    .expect_err("directory symlink cycles should be rejected before recursive copy");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_rejects_destination_inside_followed_directory_symlink_target() {
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
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
    )
    .expect_err("destination inside followed symlink target should be rejected");

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

    let error = LocalFiles::copy_dir_all_with(&src_link, &dst, LocalCopyDirOptions::default())
        .expect_err("source symlink should be rejected by default");
    assert_eq!(ErrorKind::Unsupported, error.kind());

    let stats = LocalFiles::copy_dir_all_with(
        &src_link,
        &dst,
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
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

    LocalFiles::atomic_write(&link, b"replacement").expect("symlink path should be replaced");

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
    let listener = UnixListener::bind(&socket).expect("unix socket should be created");

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
        .expect_err("socket source should be rejected");

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
    let listener = UnixListener::bind(&socket).expect("unix socket should be created");
    std::os::unix::fs::symlink(&socket, src.join("socket-link")).unwrap();

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
    )
    .expect_err("socket symlink target should be rejected");

    assert_eq!(ErrorKind::Unsupported, error.kind());
    drop(listener);
    fs::remove_dir_all(dir).unwrap();
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
    fs::set_permissions(src.join("data.txt"), fs::Permissions::from_mode(0o640)).unwrap();

    LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions {
            preserve_permissions: true,
            ..LocalCopyDirOptions::default()
        },
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
fn test_copy_dir_all_with_returns_file_copy_error() {
    let dir = temp_dir("copy-dir-file-copy-error");
    let src = dir.join("src");
    let dst = dir.join("dst");
    let file = src.join("data.txt");
    fs::create_dir(&src).unwrap();
    fs::write(&file, b"data").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
        .expect_err("unreadable source file should fail");

    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_write_with_returns_parent_error() {
    let dir = temp_dir("atomic-parent-error");
    let file_parent = dir.join("file-parent");
    fs::write(&file_parent, b"not a directory").unwrap();

    let error = LocalFiles::atomic_write_with(file_parent.join("child.txt"), |_| Ok(()))
        .expect_err("file parent should return create-dir error");

    assert!(matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory
    ));
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

    let error =
        LocalFiles::atomic_write(&path, b"data").expect_err("symlink loop metadata should fail");

    assert!(
        error
            .to_string()
            .contains("failed to read destination metadata")
    );
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
    assert!(path.is_dir());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_parent_sync_open_error_when_directory_is_not_readable() {
    let dir = temp_dir("atomic-parent-sync-open-error");
    let parent = dir.join("parent");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o300)).unwrap();

    let result = LocalFiles::atomic_write(parent.join("out.txt"), b"data");

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    if let Err(error) = result {
        assert_eq!(ErrorKind::PermissionDenied, error.kind());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_copy_dir_all_with_returns_destination_create_error() {
    let dir = temp_dir("copy-destination-create-error");
    let src = dir.join("src");
    let dst = dir.join("missing-parent").join("dst");
    fs::create_dir(&src).unwrap();

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
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

    let error = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())
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
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
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
        LocalCopyDirOptions {
            follow_symlinks: true,
            ..LocalCopyDirOptions::default()
        },
    )
    .expect_err("broken root symlink target should be reported");

    assert_eq!(ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(dir).unwrap();
}
