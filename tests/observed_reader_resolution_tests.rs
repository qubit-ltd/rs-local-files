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
