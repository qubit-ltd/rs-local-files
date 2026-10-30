// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::observation::LocalSourceVerification;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalResolutionLimits;
use qubit_local_files::options::LocalRootOpenOptions;
use tempfile::tempdir;

#[test]
fn test_child_binding_charges_inherited_anchor_paths() {
    let scratch = tempdir().expect("fixture should exist");
    let relative = std::iter::repeat_n("d", 32).collect::<Vec<_>>().join("/");
    fs::create_dir_all(scratch.path().join(&relative)).expect("deep directory should exist");
    fs::write(scratch.path().join(&relative).join("payload"), b"data").expect("payload should exist");
    let path_bytes = scratch.path().join(&relative).as_os_str().len();
    let cap = path_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(64))
        .expect("fixture budget should be representable");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let parent = filesystem
        .open_directory_with_options(
            Path::new(&relative),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("unbounded parent binding should open");

    let error = parent
        .open_directory(
            Path::new("."),
            &LocalDirectoryOpenOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(cap)),
        )
        .expect_err("the inherited anchor paths should exceed the cap");

    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
    assert!(error.path().is_some());
    parent
        .observe()
        .expect("failed child binding must leave the parent usable");

    let unbounded_child = parent
        .open_directory(
            Path::new("."),
            &LocalDirectoryOpenOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(usize::MAX)),
        )
        .expect("a sufficient cap should retain inherited evidence");
    let reader = unbounded_child
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("child binding should open its payload");
    assert_eq!(
        LocalSourceVerification::Unchanged,
        reader
            .verify_path_binding()
            .expect("inherited path evidence should verify")
    );
}

#[test]
fn test_unbound_parent_fallback_paths_are_charged() {
    let scratch = tempdir().expect("fixture should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let parent = filesystem
        .open_directory_with_options(Path::new("."), &LocalDirectoryOpenOptions::new())
        .expect("unbound parent should open");
    let error = parent
        .open_directory(
            Path::new("."),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(0)),
        )
        .expect_err("fallback anchor paths exceed a zero byte cap");

    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    assert_eq!(LocalFileOperation::OpenRoot, error.operation());
    assert!(error.path().is_some());
    parent.observe().expect("failed fallback binding leaves parent usable");
}

#[cfg(windows)]
mod support;

#[cfg(unix)]
#[test]
fn test_binding_limits_reject_directory_symlink_on_both_opening_routes() {
    let scratch = tempdir().expect("fixture should exist");
    fs::create_dir(scratch.path().join("real")).expect("target should exist");
    std::os::unix::fs::symlink("real", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let options = LocalDirectoryOpenOptions::new().with_path_binding(
        LocalPathBindingOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0)),
    );
    let error = filesystem
        .open_directory_with_options(Path::new("alias"), &options)
        .expect_err("nested zero-hop budget must reject alias");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    let parent = filesystem
        .open_directory_with_options(Path::new("."), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let error = parent
        .open_directory(Path::new("alias"), &options)
        .expect_err("child route must enforce the same budget");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn test_direct_and_binding_resolution_limits_have_the_same_effect() {
    let scratch = tempdir().expect("fixture should exist");
    fs::create_dir(scratch.path().join("real")).expect("target should exist");
    std::os::unix::fs::symlink("real", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let limits = [
        LocalResolutionLimits::new().with_max_symlink_hops(0),
        LocalResolutionLimits::new().with_max_symlink_target_bytes(0),
        LocalResolutionLimits::new().with_max_actual_components(0),
        LocalResolutionLimits::new().with_max_resolved_path_bytes(0),
    ];

    for limit in limits {
        let direct = LocalDirectoryOpenOptions::new().with_resolution_limits(limit);
        let nested = LocalDirectoryOpenOptions::new()
            .with_path_binding(LocalPathBindingOptions::new().with_resolution_limits(limit));
        let direct_error = filesystem
            .open_directory_with_options(Path::new("alias"), &direct)
            .expect_err("direct limit must reject the alias");
        let nested_error = filesystem
            .open_directory_with_options(Path::new("alias"), &nested)
            .expect_err("binding limit must reject the alias");
        assert_eq!(direct_error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(nested_error.kind(), direct_error.kind());
    }
}

#[cfg(unix)]
#[test]
fn test_directory_binding_limits_cannot_relax_parent_budget() {
    let scratch = tempdir().expect("fixture should exist");
    let root = fs::canonicalize(scratch.path()).expect("fixture root should resolve");
    fs::create_dir(root.join("real")).expect("target should exist");
    std::os::unix::fs::symlink("real", root.join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::rooted_with_options(
        &root,
        &LocalRootOpenOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0)),
    )
    .expect("root should open");
    let no_binding = filesystem
        .open_directory_with_options(Path::new("alias"), &LocalDirectoryOpenOptions::new())
        .expect_err("None must not clear the parent budget");
    let looser_binding = filesystem
        .open_directory_with_options(
            Path::new("alias"),
            &LocalDirectoryOpenOptions::new().with_path_binding(
                LocalPathBindingOptions::new()
                    .with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(10)),
            ),
        )
        .expect_err("a larger child budget must not relax the parent budget");
    assert_eq!(no_binding.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(looser_binding.kind(), no_binding.kind());
}

#[cfg(unix)]
#[test]
fn test_host_directory_open_applies_nested_binding_resolution_limits() {
    let scratch = tempdir().expect("fixture should exist");
    fs::create_dir(scratch.path().join("real")).expect("target should exist");
    std::os::unix::fs::symlink("real", scratch.path().join("alias")).expect("alias should exist");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let options = LocalDirectoryOpenOptions::new().with_path_binding(
        LocalPathBindingOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0)),
    );
    let error = filesystem
        .open_directory_with_options(&scratch.path().join("alias"), &options)
        .expect_err("Host nested zero-hop budget must reject alias");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(windows)]
#[test]
fn test_host_directory_path_binding_remains_unsupported() {
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let options = LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new());
    let error = filesystem
        .open_directory_with_options(Path::new("."), &options)
        .expect_err("Host path binding remains unsupported on Windows");
    assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);
}

#[cfg(any(unix, windows))]
#[test]
fn test_directory_binding_recheck_applies_zero_hop_limit_after_ancestor_replacement() {
    let scratch = tempdir().expect("fixture should exist");
    let original = scratch.path().join("original");
    let renamed = scratch.path().join("retained");
    fs::create_dir(&original).expect("bound directory should exist");
    fs::write(original.join("payload"), b"data").expect("payload should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let options = LocalDirectoryOpenOptions::new().with_path_binding(
        LocalPathBindingOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0)),
    );
    let directory = filesystem
        .open_directory_with_options(Path::new("original"), &options)
        .expect("unlinked path should open under a zero-hop limit");

    fs::rename(&original, &renamed).expect("bound directory should be renamed");
    #[cfg(unix)]
    std::os::unix::fs::symlink("retained", &original).expect("replacement link should be created");
    #[cfg(windows)]
    if !support::windows_symlink::create_directory_link(&renamed, &original) {
        return;
    }

    let reader = directory
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("retained authority should still open the payload");
    let error = reader
        .verify_path_binding()
        .expect_err("rechecking the replaced ancestor must enforce zero hops");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(windows)]
#[test]
fn test_rooted_directory_binding_zero_hop_limit_rejects_windows_symlink() {
    let scratch = tempdir().expect("fixture should exist");
    let target = scratch.path().join("target");
    let alias = scratch.path().join("alias");
    fs::create_dir(&target).expect("target should exist");
    if !support::windows_symlink::create_directory_link(&target, &alias) {
        return;
    }
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let options = LocalDirectoryOpenOptions::new().with_path_binding(
        LocalPathBindingOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0)),
    );
    let error = filesystem
        .open_directory_with_options(Path::new("alias"), &options)
        .expect_err("Rooted Windows nested zero-hop budget must reject the alias");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}
