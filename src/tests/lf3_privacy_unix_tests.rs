#![cfg(unix)]

use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};

use crate::temp::internal::privacy_unix;

#[test]
fn lf3_privacy_unix_private_at_creation() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");

    let file = privacy_unix::create_private_file_at(&parent, OsStr::new("private"))
        .expect("create private file");
    let mode = file
        .metadata()
        .expect("stat private file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode & 0o077, 0, "file must be private at creation");
    assert_eq!(mode & 0o600, 0o600, "owner access must survive umask");

    let directory = privacy_unix::create_private_directory_at(&parent, OsStr::new("private-dir"))
        .expect("create private directory");
    let mode = directory
        .metadata()
        .expect("stat private directory")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode & 0o077, 0, "directory must be private at creation");
    assert_eq!(mode & 0o700, 0o700, "owner access must survive umask");
}

#[test]
fn lf3_privacy_unix_rejects_symlink_and_non_regular_open_targets() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    fs::write(fixture.path().join("target"), b"target").expect("create target");
    symlink("target", fixture.path().join("link")).expect("create link");

    let error = privacy_unix::create_private_file_at(&parent, OsStr::new("link"))
        .expect_err("must not follow or claim a pre-existing symlink");
    assert!(
        error.retained.is_none(),
        "pre-existing target is not owned by this call"
    );
    assert_eq!(
        fs::read(fixture.path().join("target")).expect("read target"),
        b"target"
    );
}

#[test]
fn lf3_privacy_unix_creates_relative_to_retained_parent_after_rename() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let original = fixture.path().join("parent");
    let moved = fixture.path().join("moved-parent");
    fs::create_dir(&original).expect("create parent");
    let parent = File::open(&original).expect("open parent handle");
    fs::rename(&original, &moved).expect("rename parent");

    privacy_unix::create_private_file_at(&parent, OsStr::new("created"))
        .expect("create through retained handle");
    assert!(moved.join("created").is_file());
    assert!(!original.exists());
}

#[test]
fn lf3_privacy_unix_access_modes_are_applied_to_verified_handles() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let file =
        privacy_unix::create_private_file_at(&parent, OsStr::new("file")).expect("create file");
    let directory = privacy_unix::create_private_directory_at(&parent, OsStr::new("directory"))
        .expect("create directory");

    privacy_unix::apply_current_user_access(&file, false, true).expect("make file read-only");
    privacy_unix::verify_current_user_access(&file, false, true).expect("verify read-only file");
    assert_eq!(
        file.metadata().expect("stat file").permissions().mode() & 0o777,
        0o400
    );

    privacy_unix::apply_current_user_access(&directory, true, true)
        .expect("make directory read-only");
    privacy_unix::verify_current_user_access(&directory, true, true)
        .expect("verify read-only directory");
    assert_eq!(
        directory
            .metadata()
            .expect("stat directory")
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
}

#[test]
fn lf3_privacy_unix_fifo_is_not_followed_by_private_file_creation() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let fifo = fixture.path().join("fifo");
    let path =
        std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).expect("fifo path has no nul");
    let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(result, 0, "create FIFO");
    assert!(
        fs::symlink_metadata(&fifo)
            .expect("stat FIFO")
            .file_type()
            .is_fifo()
    );
    assert!(privacy_unix::create_private_file_at(&parent, OsStr::new("fifo")).is_err());
    assert!(
        fs::symlink_metadata(&fifo)
            .expect("restat FIFO")
            .file_type()
            .is_fifo()
    );
}
