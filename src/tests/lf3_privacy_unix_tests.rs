// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(unix)]

use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;

use crate::temp::internal::privacy_unix;

#[test]
fn lf3_privacy_unix_private_at_creation() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");

    let file = privacy_unix::create_private_file_at(&parent, OsStr::new("private")).expect("create private file");
    let mode = file.metadata().expect("stat private file").permissions().mode() & 0o777;
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
    let file_error = privacy_unix::create_private_file_at_with_clone_hook(&parent, OsStr::new("file"), |_: &File| {
        Err(std::io::Error::from_raw_os_error(libc::EMFILE))
    })
    .expect_err("file creation must fail when parent retention cannot be prepared");
    assert_eq!(file_error.source.raw_os_error(), Some(libc::EMFILE));
    assert!(file_error.retained.is_none());
    assert!(!fixture.path().join("file").exists());

    let directory_error =
        privacy_unix::create_private_directory_at_with_clone_hook(&parent, OsStr::new("directory"), |_: &File| {
            Err(std::io::Error::from_raw_os_error(libc::EMFILE))
        })
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
    assert_eq!(fs::read(fixture.path().join("target")).expect("read target"), b"target");
}

#[test]
fn lf3_privacy_unix_creates_relative_to_retained_parent_after_rename() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let original = fixture.path().join("parent");
    let moved = fixture.path().join("moved-parent");
    fs::create_dir(&original).expect("create parent");
    let parent = File::open(&original).expect("open parent handle");
    fs::rename(&original, &moved).expect("rename parent");

    privacy_unix::create_private_file_at(&parent, OsStr::new("created")).expect("create through retained handle");
    assert!(moved.join("created").is_file());
    assert!(!original.exists());
}

#[test]
fn lf3_privacy_unix_directory_swap_after_open_is_indeterminate() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let replaced_path = fixture.path().join("created");

    let error = privacy_unix::create_private_directory_at_with_hook(&parent, OsStr::new("created"), || {
        fs::remove_dir(&replaced_path).expect("remove just-created directory");
        fs::create_dir(&replaced_path).expect("install replacement directory");
        fs::set_permissions(&replaced_path, fs::Permissions::from_mode(0o700)).expect("make replacement look private");
    })
    .expect_err("must not return a handle after its name is replaced");

    assert!(replaced_path.is_dir(), "replacement must be retained");
    let retained = error.retained.expect("preserve partial entry guard");
    let replacement_metadata = fs::symlink_metadata(&replaced_path).expect("stat replacement");
    let replacement_identity =
        crate::observation::LocalFileIdentity::unix(replacement_metadata.dev(), replacement_metadata.ino());
    assert_ne!(
        retained.identity.as_ref(),
        Some(&replacement_identity),
        "guard identity must describe the opened directory, never the replacement"
    );
    let opened_directory = retained.file.as_ref().expect("retain the opened directory handle");
    privacy_unix::apply_current_user_access(opened_directory, true, true).expect("change access on retained handle");
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
    let file = privacy_unix::create_private_file_at(&parent, OsStr::new("file")).expect("create file");
    let directory =
        privacy_unix::create_private_directory_at(&parent, OsStr::new("directory")).expect("create directory");

    privacy_unix::apply_current_user_access(&file, false, true).expect("make file read-only");
    privacy_unix::verify_current_user_access(&file, false, true).expect("verify read-only file");
    assert_eq!(file.metadata().expect("stat file").permissions().mode() & 0o777, 0o400);

    privacy_unix::apply_current_user_access(&directory, true, true).expect("make directory read-only");
    privacy_unix::verify_current_user_access(&directory, true, true).expect("verify read-only directory");
    assert_eq!(
        directory.metadata().expect("stat directory").permissions().mode() & 0o777,
        0o500
    );
}

#[test]
fn lf3_privacy_unix_fifo_is_not_followed_by_private_file_creation() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let fifo = fixture.path().join("fifo");
    let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).expect("fifo path has no nul");
    let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(result, 0, "create FIFO");
    assert!(fs::symlink_metadata(&fifo).expect("stat FIFO").file_type().is_fifo());
    assert!(privacy_unix::create_private_file_at(&parent, OsStr::new("fifo")).is_err());
    assert!(fs::symlink_metadata(&fifo).expect("restat FIFO").file_type().is_fifo());
}

#[test]
fn lf3_privacy_unix_missing_name_after_open_retains_original_identity() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let original = fixture.path().join("created");
    let moved = fixture.path().join("moved");

    let error = privacy_unix::create_private_directory_at_with_hook(&parent, OsStr::new("created"), || {
        fs::rename(&original, &moved).expect("move directory after handle acquisition");
    })
    .expect_err("missing original name must fail identity verification");

    assert_eq!(error.source.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(error.source.raw_os_error(), Some(libc::ENOENT));
    let retained = error.retained.expect("preserve partially created directory");
    assert!(retained.directory);
    assert_eq!(retained.name, OsStr::new("created"));
    let opened = retained.file.as_ref().expect("retain original opened handle");
    let metadata = opened.metadata().expect("stat retained directory");
    let expected = crate::observation::LocalFileIdentity::unix(metadata.dev(), metadata.ino());
    assert_eq!(retained.identity.as_ref(), Some(&expected));
    assert_eq!(
        metadata.ino(),
        fs::metadata(&moved).expect("stat moved directory").ino()
    );
    assert_eq!(
        retained.parent.metadata().expect("retained parent").ino(),
        parent.metadata().expect("parent").ino()
    );
    assert!(!original.exists());
    assert!(moved.is_dir());
}

#[test]
fn lf3_privacy_unix_invalid_mode_after_open_preserves_partial_directory() {
    for mode in [0o770, 0o600] {
        let fixture = tempfile::tempdir().expect("create fixture");
        let parent = File::open(fixture.path()).expect("open parent");
        let path = fixture.path().join("created");
        let error = privacy_unix::create_private_directory_at_with_hook(&parent, OsStr::new("created"), || {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("alter directory mode after opening");
        })
        .expect_err("excess group access and missing owner execute must both be rejected");

        assert_eq!(error.source.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            error.source.to_string(),
            "umask prevented required private directory mode 0700"
        );
        let retained = error.retained.expect("preserve directory for identity-aware recovery");
        assert!(retained.directory);
        assert_eq!(retained.name, OsStr::new("created"));
        let opened = retained.file.as_ref().expect("retain opened directory");
        let metadata = opened.metadata().expect("retained metadata");
        assert_eq!(metadata.mode() & 0o777, mode);
        let expected = crate::observation::LocalFileIdentity::unix(metadata.dev(), metadata.ino());
        assert_eq!(retained.identity.as_ref(), Some(&expected));
        assert_eq!(
            metadata.ino(),
            fs::metadata(&path).expect("original name remains").ino()
        );
        privacy_unix::apply_current_user_access(opened, true, false).expect("repair access through retained handle");
        privacy_unix::verify_current_user_access(opened, true, false).expect("verify repaired directory");
    }
}

#[test]
fn lf3_privacy_unix_missing_name_before_open_keeps_identity_unproven() {
    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let original = fixture.path().join("created");
    let moved = fixture.path().join("moved");
    let error =
        privacy_unix::create_private_directory_at_with_open_failure_hook(&parent, OsStr::new("created"), || {
            fs::rename(&original, &moved)
        })
        .expect_err("native open must fail after original name disappears");

    assert_eq!(error.source.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(error.source.raw_os_error(), Some(libc::ENOENT));
    let retained = error.retained.expect("preserve partial creation context");
    assert!(retained.directory);
    assert_eq!(retained.name, OsStr::new("created"));
    assert!(retained.file.is_none(), "no handle was acquired");
    assert!(retained.identity.is_none(), "no identity was proven");
    assert!(!original.exists());
    assert!(moved.is_dir());
}

#[test]
fn lf3_privacy_unix_nul_names_fail_before_creation() {
    use std::os::unix::ffi::OsStrExt;

    let fixture = tempfile::tempdir().expect("create fixture");
    let parent = File::open(fixture.path()).expect("open parent");
    let name = OsStr::from_bytes(b"invalid\0name");
    for directory in [false, true] {
        let result = if directory {
            privacy_unix::create_private_directory_at(&parent, name)
        } else {
            privacy_unix::create_private_file_at(&parent, name)
        };
        let error = result.expect_err("native component containing NUL is rejected");
        assert_eq!(error.source.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(error.source.to_string(), "name contains a nul byte");
        assert!(error.retained.is_none());
        assert_eq!(fs::read_dir(fixture.path()).expect("inspect parent").count(), 0);
    }
}

#[test]
fn lf3_privacy_unix_sandbox_metadata_failure_retains_verified_recovery_owner() {
    verify_private_creation_metadata_failure(1);
}

#[test]
fn lf3_privacy_unix_resource_metadata_failure_retains_child_and_sandbox_owners() {
    verify_private_creation_metadata_failure(2);
}

fn verify_private_creation_metadata_failure(occurrence: usize) {
    use std::path::Path;

    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTempFileOptions;
    use crate::policy::LocalPrivacyRequirement;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    for rooted in [false, true] {
        for directory in [false, true] {
            let fixture = tempfile::tempdir().expect("private creation fixture");
            let filesystem = if rooted {
                LocalFileSystem::rooted(fixture.path()).expect("rooted authority")
            } else {
                LocalFileSystem::host().expect("host authority")
            };
            let parent = if rooted { Path::new("/") } else { fixture.path() };
            let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::HandleMetadata, occurrence);
            let error = if directory {
                filesystem
                    .create_temp_directory_with_options(
                        &LocalTempDirectoryOptions::new()
                            .with_parent(parent)
                            .with_privacy(LocalPrivacyRequirement::Required),
                    )
                    .expect_err("private directory metadata failure")
            } else {
                filesystem
                    .create_temp_file_with_options(
                        &LocalTempFileOptions::new()
                            .with_parent(parent)
                            .with_privacy(LocalPrivacyRequirement::Required),
                    )
                    .expect_err("private file metadata failure")
            };
            assert!(guard.fired());
            assert_eq!(error.kind(), LocalFileErrorKind::Io);
            assert_eq!(
                error.operation(),
                if directory {
                    LocalFileOperation::CreateTempDirectory
                } else {
                    LocalFileOperation::CreateTempFile
                }
            );
            assert_eq!(
                error.io_error().expect("native metadata failure").to_string(),
                "injected native I/O contract failure"
            );
            let (_, recovery) = error.into_temp_recovery_parts();
            let mut recovery = recovery.expect("partial private creation transfers cleanup ownership");
            let sandbox = fs::read_dir(fixture.path())
                .expect("retained sandbox listing")
                .next()
                .expect("one retained sandbox")
                .expect("sandbox entry")
                .path();
            let target = if occurrence == 1 {
                assert_eq!(fs::read_dir(&sandbox).expect("sandbox children").count(), 0);
                sandbox.clone()
            } else {
                fs::read_dir(&sandbox)
                    .expect("retained child listing")
                    .next()
                    .expect("retained resource child")
                    .expect("resource entry")
                    .path()
            };
            let metadata = fs::metadata(&target).expect("retained native resource");
            assert_eq!(metadata.is_dir(), occurrence == 1 || directory);
            let identity = crate::observation::LocalFileIdentity::unix(metadata.dev(), metadata.ino());
            assert_eq!(
                recovery.identity(),
                Some(&identity),
                "identity comes from the still-valid retained handle"
            );
            drop(guard);
            recovery
                .cleanup()
                .expect("retry observes valid retained handle and removes child plus sandbox");
            assert!(!target.exists());
            assert!(!sandbox.exists());
            assert_eq!(fs::read_dir(fixture.path()).expect("parent after cleanup").count(), 0);
            recovery.cleanup().expect("completed recovery cleanup is idempotent");
        }
    }
}

#[test]
fn lf3_privacy_unix_sandbox_random_name_failure_precedes_private_mutation() {
    verify_private_random_name_failure(1);
}

#[test]
fn lf3_privacy_unix_resource_random_name_failure_retains_empty_sandbox_owner() {
    verify_private_random_name_failure(2);
}

fn verify_private_random_name_failure(occurrence: usize) {
    use std::path::Path;

    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTempFileOptions;
    use crate::policy::LocalPrivacyRequirement;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    for rooted in [false, true] {
        for directory in [false, true] {
            let fixture = tempfile::tempdir().expect("private random-name fixture");
            let filesystem = if rooted {
                LocalFileSystem::rooted(fixture.path()).expect("rooted authority")
            } else {
                LocalFileSystem::host().expect("host authority")
            };
            let parent = if rooted {
                Path::new("/").to_path_buf()
            } else {
                fs::canonicalize(fixture.path()).expect("fixture path should canonicalize")
            };
            let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::RandomName, occurrence);
            let error = if directory {
                filesystem
                    .create_temp_directory_with_options(
                        &LocalTempDirectoryOptions::new()
                            .with_parent(&parent)
                            .with_privacy(LocalPrivacyRequirement::Required),
                    )
                    .expect_err("private directory random-name failure")
            } else {
                filesystem
                    .create_temp_file_with_options(
                        &LocalTempFileOptions::new()
                            .with_parent(&parent)
                            .with_privacy(LocalPrivacyRequirement::Required),
                    )
                    .expect_err("private file random-name failure")
            };
            assert!(guard.fired());
            assert_eq!(error.kind(), LocalFileErrorKind::Io);
            assert_eq!(
                error.operation(),
                if directory {
                    LocalFileOperation::CreateTempDirectory
                } else {
                    LocalFileOperation::CreateTempFile
                }
            );
            let native = error.io_error().expect("native random generation error");
            assert_eq!(native.kind(), std::io::ErrorKind::Other);
            assert_eq!(
                native
                    .get_ref()
                    .expect("getrandom error")
                    .downcast_ref::<getrandom::Error>(),
                Some(&getrandom::Error::UNSUPPORTED)
            );
            let error_path = error.path().expect("creation diagnostic path").to_path_buf();
            let (_, recovery) = error.into_temp_recovery_parts();
            if occurrence == 1 {
                assert_eq!(error_path, parent);
                assert!(recovery.is_none(), "nothing was created before sandbox name generation");
                assert_eq!(fs::read_dir(fixture.path()).expect("unchanged parent").count(), 0);
            } else {
                let mut recovery = recovery.expect("created sandbox retains cleanup ownership");
                let sandbox = fs::read_dir(fixture.path())
                    .expect("sandbox listing")
                    .next()
                    .expect("retained sandbox")
                    .expect("sandbox entry")
                    .path();
                let sandbox = fs::canonicalize(sandbox).expect("sandbox path should canonicalize");
                assert_eq!(
                    error_path, parent,
                    "public diagnostics retain the requested creation parent"
                );
                let recovery_path = if rooted {
                    Path::new(sandbox.file_name().expect("sandbox name")).to_path_buf()
                } else {
                    sandbox.clone()
                };
                assert_eq!(recovery.path(), recovery_path);
                assert_eq!(fs::read_dir(&sandbox).expect("empty sandbox").count(), 0);
                let metadata = fs::metadata(&sandbox).expect("sandbox metadata");
                let identity = crate::observation::LocalFileIdentity::unix(metadata.dev(), metadata.ino());
                assert_eq!(recovery.identity(), Some(&identity));
                drop(guard);
                recovery.cleanup().expect("cleanup valid retained empty sandbox");
                assert!(!sandbox.exists());
                assert_eq!(fs::read_dir(fixture.path()).expect("parent after recovery").count(), 0);
                continue;
            }
            drop(guard);
        }
    }
}

#[test]
fn lf3_privacy_unix_public_filename_generation_preserves_entropy_failure() {
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::path::LocalFileNames;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    let names = LocalFileNames::portable();
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::RandomName, 1);
    let error = names
        .random_name_with(Some(OsStr::new("report-")), Some(OsStr::new(".csv")))
        .expect_err("native randomness failure prevents generating a public filename");
    assert!(guard.fired());
    assert_eq!(error.kind(), LocalFileErrorKind::Io);
    assert_eq!(error.operation(), LocalFileOperation::GenerateName);
    assert!(error.path().is_none());
    assert_eq!(
        error
            .io_error()
            .expect("entropy source")
            .get_ref()
            .expect("getrandom failure")
            .downcast_ref::<getrandom::Error>(),
        Some(&getrandom::Error::UNSUPPORTED)
    );
    drop(guard);
    let generated = names
        .random_name_with(Some(OsStr::new("report-")), Some(OsStr::new(".csv")))
        .expect("filename generation retries normally after transient entropy failure");
    let generated = generated.to_str().expect("portable ASCII filename");
    assert!(generated.starts_with("report-"));
    assert!(generated.ends_with(".csv"));
}
