// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalResolutionLimits;

#[cfg(windows)]
mod support;

#[test]
fn host_rejects_finite_resolution_limits_before_resolving_the_path() {
    let scratch = tempfile::tempdir().expect("scratch directory should be created");
    let missing = scratch.path().join("missing");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let limits = [
        LocalResolutionLimits::new().with_max_symlink_hops(0),
        LocalResolutionLimits::new().with_max_symlink_hops(1),
        LocalResolutionLimits::new().with_max_symlink_target_bytes(0),
        LocalResolutionLimits::new().with_max_symlink_target_bytes(1),
        LocalResolutionLimits::new().with_max_actual_components(0),
        LocalResolutionLimits::new().with_max_actual_components(1),
        LocalResolutionLimits::new().with_max_resolved_path_bytes(0),
        LocalResolutionLimits::new().with_max_resolved_path_bytes(1),
    ];

    for limits in limits {
        let options = LocalObservedReadOptions::new().with_resolution_limits(limits);
        let error = filesystem
            .open_observed_reader(&missing, &options)
            .expect_err("Host must reject finite resolution limits before path lookup");
        assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);
        assert_eq!(error.path(), Some(missing.as_path()));
    }
}

#[test]
fn host_accepts_absent_and_empty_resolution_limits() {
    let scratch = tempfile::tempdir().expect("scratch directory should be created");
    let file = scratch.path().join("payload");
    std::fs::write(&file, b"data").expect("payload should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");

    for options in [
        LocalObservedReadOptions::new(),
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new()),
    ] {
        let mut reader = filesystem
            .open_observed_reader(&file, &options)
            .expect("unbounded Host reads should remain supported");
        let mut contents = Vec::new();
        Streams::copy_input_to_output(&mut reader, &mut contents).expect("payload should be readable");
        assert_eq!(contents, b"data");
    }
}

#[cfg(unix)]
#[test]
fn host_binding_rejects_finite_resolution_limits() {
    use qubit_local_files::options::LocalPathBindingOptions;

    let scratch = tempfile::tempdir().expect("scratch directory should be created");
    let file = scratch.path().join("payload");
    std::fs::write(&file, b"data").expect("payload should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let binding =
        LocalPathBindingOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));
    let options = LocalObservedReadOptions::new().with_path_binding(binding);

    let error = filesystem
        .open_observed_reader(&file, &options)
        .expect_err("Host binding must not silently ignore finite resolution limits");
    assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);
    assert_eq!(error.path(), Some(file.as_path()));
}

#[cfg(unix)]
#[test]
fn rooted_reads_still_enforce_zero_symlink_hops() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    let scratch = tempfile::tempdir().expect("scratch directory should be created");
    std::fs::write(scratch.path().join("payload"), b"data").expect("payload should be created");
    symlink("payload", scratch.path().join("alias")).expect("symbolic link should be created");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let options =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));

    let error = filesystem
        .open_observed_reader(Path::new("alias"), &options)
        .expect_err("Rooted resolution must enforce the zero-hop limit");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(windows)]
#[test]
fn test_fixed_observed_read_enforces_zero_symlink_hops() {
    use std::path::Path;

    use qubit_local_files::options::LocalDirectoryOpenOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    let payload = scratch.path().join("payload");
    let link = scratch.path().join("link");
    std::fs::write(&payload, b"data").expect("payload should exist");
    if !support::windows_symlink::create_file_link(&payload, &link) {
        return;
    }
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    let options =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));
    let error = directory
        .open_observed_reader(Path::new("link"), &options)
        .expect_err("fixed observed read must count the original link");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(
        error.resource_limit_error().map(|limit| limit.resource()),
        Some(qubit_local_files::error::LocalResourceKind::SymlinkHops)
    );
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_read_enforces_zero_symlink_hops() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::options::LocalDirectoryOpenOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(scratch.path().join("payload"), b"data").expect("payload should exist");
    symlink("payload", scratch.path().join("link")).expect("link should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    let options =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));
    let error = directory
        .open_observed_reader(Path::new("link"), &options)
        .expect_err("fixed observed read must count the original link");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(
        error.resource_limit_error().map(|limit| limit.resource()),
        Some(qubit_local_files::error::LocalResourceKind::SymlinkHops)
    );
    let mut reader = directory
        .open_observed_reader(Path::new("link"), &LocalObservedReadOptions::new())
        .expect("unbounded fixed read should follow the link");
    let mut contents = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut contents).expect("contents should be readable");
    assert_eq!(contents, b"data");
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_binding_detects_child_link_retarget() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::observation::LocalSourceVerification;
    use qubit_local_files::options::LocalDirectoryOpenOptions;
    use qubit_local_files::options::LocalPathBindingOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(scratch.path().join("payload"), b"same object").expect("payload should exist");
    std::fs::write(scratch.path().join("other"), b"other object").expect("other should exist");
    symlink("payload", scratch.path().join("link")).expect("link should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    let options = LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new());
    let reader = directory
        .open_observed_reader(Path::new("link"), &options)
        .expect("reader should open");
    std::fs::remove_file(scratch.path().join("link")).expect("old link should be removed");
    symlink("other", scratch.path().join("link")).expect("retargeted link should exist");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Changed
    );
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_binding_detects_ancestor_link_retarget() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::observation::LocalSourceVerification;
    use qubit_local_files::options::LocalDirectoryOpenOptions;
    use qubit_local_files::options::LocalPathBindingOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir(scratch.path().join("first")).expect("first directory should exist");
    std::fs::create_dir(scratch.path().join("second")).expect("second directory should exist");
    std::fs::write(scratch.path().join("first/payload"), b"same payload").expect("payload should exist");
    std::fs::write(scratch.path().join("second/payload"), b"other payload").expect("other should exist");
    symlink("first", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    let reader = directory
        .open_observed_reader(
            Path::new("alias/payload"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader should open");
    std::fs::remove_file(scratch.path().join("alias")).expect("old alias should be removed");
    symlink("second", scratch.path().join("alias")).expect("retargeted alias should exist");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Changed
    );
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_binding_reports_deleted_original_name_as_missing() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::observation::LocalSourceVerification;
    use qubit_local_files::options::LocalDirectoryOpenOptions;
    use qubit_local_files::options::LocalPathBindingOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(scratch.path().join("payload"), b"data").expect("payload should exist");
    symlink("payload", scratch.path().join("link")).expect("link should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    let reader = directory
        .open_observed_reader(
            Path::new("link"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader should open");
    std::fs::remove_file(scratch.path().join("link")).expect("original name should be removed");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Missing
    );
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_read_checks_components_before_parent_collapse() {
    use std::ffi::OsString;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::directory::LocalComponentPolicy;
    use qubit_local_files::options::LocalDirectoryOpenOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(scratch.path().join("payload"), b"data").expect("payload should exist");
    std::fs::write(scratch.path().join("blocked"), b"denied").expect("blocked target should exist");
    symlink("blocked", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new().with_component_policy(
                LocalComponentPolicy::deny_names(vec![OsString::from("blocked")])
                    .expect("component policy should be valid"),
            ),
        )
        .expect("fixed root should open");

    let denied = directory
        .open_observed_reader(Path::new("alias"), &LocalObservedReadOptions::new())
        .expect_err("a denied name reached through an alias must remain denied");
    assert_eq!(denied.kind(), LocalFileErrorKind::InvalidPath);

    let missing = directory
        .open_observed_reader(Path::new("missing/../payload"), &LocalObservedReadOptions::new())
        .expect_err("resolution must visit the missing component before '..'");
    assert_eq!(missing.kind(), LocalFileErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_read_cannot_relax_parent_resolution_budget() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::options::LocalDirectoryOpenOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(scratch.path().join("payload"), b"data").expect("payload should exist");
    symlink("payload", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new()
                .with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0)),
        )
        .expect("fixed root should open");
    let options =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(4));
    let error = directory
        .open_observed_reader(Path::new("alias"), &options)
        .expect_err("child limits must not relax the parent contract");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_binding_enforces_zero_hops() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::options::LocalDirectoryOpenOptions;
    use qubit_local_files::options::LocalPathBindingOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(scratch.path().join("payload"), b"data").expect("payload should exist");
    symlink("payload", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    let binding =
        LocalPathBindingOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));
    let error = directory
        .open_observed_reader(
            Path::new("alias"),
            &LocalObservedReadOptions::new().with_path_binding(binding),
        )
        .expect_err("binding must count links in the original recipe");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_binding_target_byte_limit_has_exact_boundary() {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::options::LocalDirectoryOpenOptions;
    use qubit_local_files::options::LocalPathBindingOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir(scratch.path().join("target")).expect("target directory should exist");
    std::fs::write(scratch.path().join("target/payload"), b"data").expect("payload should exist");
    symlink("target/payload", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");

    let exact = (0..128)
        .find(|maximum| {
            directory
                .open_observed_reader(
                    Path::new("alias"),
                    &LocalObservedReadOptions::new()
                        .with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(*maximum)),
                )
                .is_ok()
        })
        .expect("a finite target-byte boundary should be found");
    assert!(exact > 0, "the original path and target must consume bytes");
    let below = directory
        .open_observed_reader(
            Path::new("alias"),
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(exact - 1)),
        )
        .expect_err("one byte below the measured boundary must fail");
    assert_eq!(below.kind(), LocalFileErrorKind::ResourceLimit);
    directory
        .open_observed_reader(
            Path::new("alias"),
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(exact)),
        )
        .expect("the measured exact boundary must succeed");
}

#[cfg(unix)]
#[test]
fn test_nested_fixed_observed_reader_uses_nested_authority() {
    use std::path::Path;

    use qubit_local_files::options::LocalDirectoryOpenOptions;

    let scratch = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir_all(scratch.path().join("outer/inner")).expect("nested directories should exist");
    std::fs::write(scratch.path().join("outer/inner/payload"), b"nested").expect("payload should exist");
    std::fs::write(scratch.path().join("outer/payload"), b"outer").expect("outer payload should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let outer = filesystem
        .open_directory_with_options(Path::new("/outer"), &LocalDirectoryOpenOptions::new())
        .expect("outer handle should open");
    let inner = outer
        .open_directory(Path::new("inner"), &LocalDirectoryOpenOptions::new())
        .expect("nested handle should open");
    let mut reader = inner
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("nested relative path should resolve under nested authority");
    let mut contents = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut contents).expect("nested payload should be readable");
    assert_eq!(contents, b"nested");
    let error = inner
        .open_observed_reader(Path::new("../payload"), &LocalObservedReadOptions::new())
        .expect_err("parent steps must not escape the nested authority");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}

#[cfg(unix)]
#[test]
fn test_fixed_observed_read_survives_diagnostic_root_rename() {
    use std::path::Path;

    use qubit_local_files::options::LocalDirectoryOpenOptions;

    let parent = tempfile::tempdir().expect("parent should be created");
    let original = parent.path().join("original");
    let renamed = parent.path().join("renamed");
    std::fs::create_dir(&original).expect("root should be created");
    std::fs::write(original.join("payload"), b"retained").expect("payload should exist");
    let filesystem = LocalFileSystem::rooted(&original).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("fixed root should open");
    std::fs::rename(&original, &renamed).expect("diagnostic root should be renamed");
    let mut reader = directory
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("retained authority should survive diagnostic rename");
    let mut contents = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut contents).expect("payload should be readable");
    assert_eq!(contents, b"retained");
    std::fs::remove_dir_all(renamed).expect("renamed fixture should be cleaned");
}
