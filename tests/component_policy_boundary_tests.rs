// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsString;
use std::fs;
use std::path::Path;

#[cfg(unix)]
use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalComponentPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
#[cfg(unix)]
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

fn policy_handle(root: &Path) -> qubit_local_files::directory::LocalDirectoryHandle {
    LocalFileSystem::rooted(root)
        .expect("Rooted filesystem should open")
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
                .with_component_policy(
                    LocalComponentPolicy::deny_names(vec![OsString::from("blocked")]).expect("policy should be valid"),
                ),
        )
        .expect("policy handle should open")
}

#[test]
fn test_component_policy_checks_direct_paths_but_not_recursive_discovery() {
    let fixture = tempdir().expect("fixture directory should exist");
    let allowed = fixture.path().join("allowed");
    fs::create_dir_all(allowed.join("blocked")).expect("blocked subtree should exist");
    fs::create_dir_all(allowed.join("visible")).expect("visible subtree should exist");
    fs::write(allowed.join("blocked/secret"), b"blocked bytes").expect("blocked payload should exist");
    fs::write(allowed.join("visible/payload"), b"visible bytes").expect("visible payload should exist");

    let handle = policy_handle(fixture.path());
    let direct_error = handle
        .entry_metadata(Path::new("allowed/blocked/secret"))
        .expect_err("direct paths containing a denied name should be rejected");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::InvalidPath,
        direct_error.kind()
    );

    let entries = handle
        .list_with_options(Path::new("allowed"), &LocalListOptions::new().with_recursive())
        .expect("the allowed listing root should bind")
        .map(|entry| entry.expect("recursive traversal should complete"))
        .map(|entry| entry.relative_path().to_path_buf())
        .collect::<Vec<_>>();
    assert!(
        entries.iter().any(|path| path == Path::new("blocked/secret")),
        "recursive discovery should include the denied-name descendant: {entries:?}"
    );

    let _result = handle
        .copy_with_options(
            Path::new("allowed"),
            Path::new("copy"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("the allowed source and destination paths should bind");
    assert_eq!(
        b"blocked bytes",
        fs::read(fixture.path().join("copy/blocked/secret"))
            .expect("recursive copy should discover the denied-name descendant")
            .as_slice()
    );
}

#[cfg(unix)]
#[test]
fn test_component_policy_allows_an_allowed_symlink_name() {
    use std::os::unix::fs::symlink;

    let fixture = tempdir().expect("fixture directory should exist");
    fs::create_dir(fixture.path().join("target")).expect("symlink target should exist");
    fs::write(fixture.path().join("target/payload"), b"symlink bytes").expect("symlink payload should exist");
    symlink("target", fixture.path().join("allowed-link")).expect("allowed symlink should be created");

    let handle = policy_handle(fixture.path());
    let mut reader = handle
        .open_observed_reader(Path::new("allowed-link/payload"), &LocalObservedReadOptions::new())
        .expect("an allowed link name and allowed target should resolve");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("linked payload should be readable");
    assert_eq!(b"symlink bytes", bytes.as_slice());
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
