// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Behavioral coverage for synchronous local file readers.

use std::io::IoSliceMut;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_io::Input;
use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalReadAllOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalSnapshotOptions;
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

/// Verifies explicit reader policies override Host and Rooted instance policies.
#[cfg(unix)]
#[test]
fn test_read_policy_reject_override_in_host_and_rooted() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    let fixture = tempdir().expect("temporary directory should be created");
    std::fs::write(fixture.path().join("target"), b"payload").expect("target should be written");
    symlink("target", fixture.path().join("link")).expect("link should be created");
    let host = LocalFileSystem::host().expect("Host filesystem should open");
    let mut rooted =
        LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem should open");
    rooted
        .set_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted policy should be configured");
    let cases = [
        (host, fixture.path().join("link")),
        (rooted, Path::new("link").to_path_buf()),
    ];
    for (filesystem, path) in cases {
        let options = LocalReadOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject);
        let error = filesystem
            .open_reader_with_options(&path, &options)
            .expect_err("explicit Reject should reject a symbolic link");
        assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);
    }
}

/// Verifies all reader entry points honor the selected policy precedence.
#[cfg(unix)]
#[test]
fn test_read_policy_override_precedence_across_read_apis() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    let fixture = tempdir().expect("temporary directory should be created");
    std::fs::write(fixture.path().join("target"), b"payload").expect("target should be written");
    symlink("target", fixture.path().join("link")).expect("link should be created");
    let mut filesystem =
        LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("Rooted policy should be configured");
    let path = Path::new("link");
    let follow = LocalReadOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope);

    filesystem
        .set_default_read_options(follow)
        .expect("default reader options should be configured");
    assert_eq!(
        filesystem
            .read_prefix(path, 7)
            .expect("default policy should follow"),
        b"payload"
    );
    assert_eq!(
        filesystem
            .read_text_with_options(
                path,
                &LocalReadAllOptions::new()
                    .with_read_options(LocalReadOptions::new())
                    .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
            )
            .expect("outer policy should override nested options"),
        "payload"
    );

    let mut reader = filesystem
        .open_reader_with_options(path, &follow)
        .expect("option override should follow");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("reader should return payload");
    assert_eq!(bytes, b"payload");
    assert_eq!(
        filesystem
            .read_prefix_with_options(path, 0, &follow)
            .expect("zero prefix should still open link"),
        b""
    );
    assert_eq!(
        filesystem
            .read_all_with_options(path, &LocalReadAllOptions::new().with_read_options(follow))
            .expect("nested override should follow"),
        b"payload"
    );
    assert_eq!(
        filesystem
            .read_all_with_options(
                path,
                &LocalReadAllOptions::new()
                    .with_read_options(LocalReadOptions::new())
                    .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
            )
            .expect("outer override should win"),
        b"payload"
    );
    assert_eq!(
        filesystem
            .read_consistent_with_options(
                path,
                &LocalSnapshotOptions::builder(64)
                    .read_options(follow)
                    .build()
            )
            .expect("snapshot override should follow")
            .bytes(),
        b"payload"
    );
    assert_eq!(
        filesystem
            .read_consistent_with_options(
                path,
                &LocalSnapshotOptions::builder(64)
                    .read_options(
                        LocalReadOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject)
                    )
                    .symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
                    .build()
            )
            .expect("snapshot outer policy should win")
            .bytes(),
        b"payload"
    );

    let reject = LocalReadOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject);
    let error = filesystem
        .open_observed_reader(
            path,
            &LocalObservedReadOptions::new().with_read_options(reject),
        )
        .expect_err("observed reader should honor nested policy");
    assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);

    let observed = filesystem
        .open_observed_reader(
            path,
            &LocalObservedReadOptions::new()
                .with_read_options(follow)
                .with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("observed reader should apply the nested policy to binding");
    assert_eq!(observed.observation().metadata().len(), 7);

    let directory = filesystem
        .open_directory_with_options(
            Path::new("."),
            &LocalDirectoryOpenOptions::new()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        )
        .expect("directory handle should open");
    let directory_reader = directory
        .open_observed_reader(
            path,
            &LocalObservedReadOptions::new().with_read_options(follow),
        )
        .expect("directory handle should use the read option policy");
    assert_eq!(directory_reader.observation().metadata().len(), 7);
}

/// Verifies Rooted scope rejects an outer cross-scope policy before reading.
#[cfg(unix)]
#[test]
fn test_read_policy_rooted_rejects_outer_cross_scope() {
    use std::path::Path;

    let fixture = tempdir().expect("temporary directory should be created");
    std::fs::write(fixture.path().join("payload"), b"payload").expect("file should be written");
    let filesystem =
        LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem should open");
    let options =
        LocalReadAllOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope);
    let error = filesystem
        .read_all_with_options(Path::new("payload"), &options)
        .expect_err("cross-scope override is invalid");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
}

/// Verifies readers expose the native handle and support sequential seeking.
#[test]
fn test_local_file_reader_reads_and_seeks() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"abcdef").expect("fixture should be written");

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&path, &LocalReadOptions::new())
        .expect("regular file should open for reading");
    assert!(
        reader
            .as_file()
            .metadata()
            .expect("handle metadata should load")
            .is_file()
    );

    let mut first = [0_u8; 2];
    let count = reader.read(&mut first).expect("prefix should be readable");
    assert_eq!(count, first.len());
    assert_eq!(&first, b"ab");

    reader.seek(SeekFrom::Start(3)).expect("seek should succeed");
    let mut last = [0_u8; 3];
    Input::read_exactly(&mut reader, &mut last).expect("suffix should be readable");
    assert_eq!(&last, b"def");
}

/// Verifies vectored reads fill the returned buffer prefix in order.
#[test]
fn test_local_file_reader_supports_vectored_reads() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"a").expect("fixture should be written");

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&path, &LocalReadOptions::new())
        .expect("regular file should open for reading");
    let mut first = [0_u8; 2];
    let mut second = [0_u8; 4];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count = reader
        .read_vectored(&mut buffers)
        .expect("vectored read should succeed");

    assert!(count > 0);
    assert!(count <= first.len() + second.len());
    let bytes = first.iter().chain(second.iter()).copied().collect::<Vec<_>>();
    assert_eq!(&bytes[..count], b"a");
}

/// Verifies Windows Host readers follow final links under the default policy.
#[cfg(windows)]
#[test]
fn test_local_file_reader_follows_final_file_symlink_on_windows() {
    use std::io::ErrorKind;
    use std::os::windows::fs::symlink_file;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    std::fs::write(&target, b"payload").expect("target should be written");
    if let Err(error) = symlink_file(&target, &link) {
        if error.kind() == ErrorKind::PermissionDenied {
            return;
        }
        panic!("file symlink should be created: {error}");
    }

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&link, &LocalReadOptions::new())
        .expect("the default Host policy should follow a final file symlink");
    let mut payload = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut payload).expect("linked file contents should be readable");
    assert_eq!(b"payload", payload.as_slice());
}
