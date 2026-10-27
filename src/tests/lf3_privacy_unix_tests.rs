#![cfg(unix)]

use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

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
fn lf3_privacy_unix_parent_clone_failure_precedes_mutation() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let file_error = privacy_unix::create_private_file_at_with_clone_hook(
        &parent,
        OsStr::new("file"),
        |_: &File| Err(std::io::Error::from_raw_os_error(libc::EMFILE)),
    )
    .expect_err("file creation must fail when parent retention cannot be prepared");
    assert_eq!(file_error.source.raw_os_error(), Some(libc::EMFILE));
    assert!(file_error.retained.is_none());
    assert!(!fixture.path().join("file").exists());

    let directory_error = privacy_unix::create_private_directory_at_with_clone_hook(
        &parent,
        OsStr::new("directory"),
        |_: &File| Err(std::io::Error::from_raw_os_error(libc::EMFILE)),
    )
    .expect_err("directory creation must fail when parent retention cannot be prepared");
    assert_eq!(directory_error.source.raw_os_error(), Some(libc::EMFILE));
    assert!(directory_error.retained.is_none());
    assert!(!fixture.path().join("directory").exists());
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
fn lf3_privacy_unix_directory_swap_after_open_is_indeterminate() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let replaced_path = fixture.path().join("created");

    let error =
        privacy_unix::create_private_directory_at_with_hook(&parent, OsStr::new("created"), || {
            fs::remove_dir(&replaced_path).expect("remove just-created directory");
            fs::create_dir(&replaced_path).expect("install replacement directory");
            fs::set_permissions(&replaced_path, fs::Permissions::from_mode(0o700))
                .expect("make replacement look private");
        })
        .expect_err("must not return a handle after its name is replaced");

    assert!(replaced_path.is_dir(), "replacement must be retained");
    let retained = error.retained.expect("preserve partial entry guard");
    let replacement_metadata = fs::symlink_metadata(&replaced_path).expect("stat replacement");
    let replacement_identity = crate::observation::LocalFileIdentity::unix(
        replacement_metadata.dev(),
        replacement_metadata.ino(),
    );
    assert_ne!(
        retained.identity.as_ref(),
        Some(&replacement_identity),
        "guard identity must describe the opened directory, never the replacement"
    );
    let opened_directory = retained
        .file
        .as_ref()
        .expect("retain the opened directory handle");
    privacy_unix::apply_current_user_access(opened_directory, true, true)
        .expect("change access on retained handle");
    assert_eq!(
        fs::symlink_metadata(&replaced_path)
            .expect("restat replacement")
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "handle access changes must leave the replacement untouched"
    );
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
