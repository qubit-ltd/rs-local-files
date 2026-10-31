// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::observation::LocalSourceVerification;
#[cfg(unix)]
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
#[cfg(unix)]
use qubit_local_files::options::LocalReadOptions;
#[cfg(unix)]
use qubit_local_files::options::LocalResolutionLimits;

#[cfg(windows)]
mod support;
#[cfg(windows)]
use support::windows_symlink::create_file_link;

#[cfg(unix)]
#[test]
fn binding_reopen_replays_the_root_contract_from_its_original_name() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("dir")).expect("bound directory should be created");
    std::fs::create_dir(tmp.path().join("other")).expect("replacement target should be created");
    std::fs::write(tmp.path().join("dir/source"), b"source").expect("source should be created");
    std::fs::write(tmp.path().join("other/source"), b"replacement").expect("replacement should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let binding =
        LocalPathBindingOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0));
    let reader_options = LocalObservedReadOptions::new().with_path_binding(binding);
    let reader = filesystem
        .open_observed_reader(std::path::Path::new("dir/source"), &reader_options)
        .expect("reader and binding should open");

    std::fs::rename(tmp.path().join("dir"), tmp.path().join("saved"))
        .expect("original bound directory should be renamed");
    symlink("other", tmp.path().join("dir")).expect("replacement directory symlink should be created");

    let error = reader
        .verify_path_binding()
        .expect_err("reopening a replaced ancestor must replay the zero-hop contract");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn binding_reports_unchanged_before_namespace_replacement() {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("dir")).expect("directory should be created");
    std::fs::write(tmp.path().join("dir/source"), b"source").expect("source should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let reader = filesystem
        .open_observed_reader(
            std::path::Path::new("dir/source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader and binding should open");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Unchanged,
    );
}

#[cfg(unix)]
#[test]
fn binding_reuses_explicit_read_symlink_policy() {
    use std::os::unix::fs::symlink;

    use qubit_local_files::policy::LocalSymlinkPolicy;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::write(tmp.path().join("payload"), b"payload").expect("payload should be created");
    symlink("payload", tmp.path().join("link")).expect("file symlink should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let options = LocalObservedReadOptions::new()
        .with_read_options(LocalReadOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope))
        .with_path_binding(LocalPathBindingOptions::new());
    let reader = filesystem
        .open_observed_reader(std::path::Path::new("link"), &options)
        .expect("explicit read policy should follow the link");

    assert_eq!(
        reader
            .verify_path_binding()
            .expect("binding should retain the explicit read policy"),
        LocalSourceVerification::Unchanged,
    );
}

#[cfg(unix)]
#[test]
fn binding_reuses_per_read_symlink_hop_limit() {
    use std::os::unix::fs::symlink;

    use qubit_local_files::policy::LocalSymlinkPolicy;

    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::write(tmp.path().join("payload"), b"payload").expect("payload should be created");
    symlink("payload", tmp.path().join("link")).expect("one-hop link should be created");
    symlink("payload", tmp.path().join("second-link")).expect("second link should be created");
    let mut filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted policy should follow in-scope links");
    let options = LocalObservedReadOptions::new()
        .with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(1))
        .with_path_binding(LocalPathBindingOptions::new());
    let reader = filesystem
        .open_observed_reader(std::path::Path::new("link"), &options)
        .expect("one-hop link should fit the per-read limit");

    std::fs::remove_file(tmp.path().join("link")).expect("original one-hop link should be removed");
    symlink("second-link", tmp.path().join("link")).expect("two-hop link chain should be installed");
    let error = reader
        .verify_path_binding()
        .expect_err("reopening should preserve the one-hop per-read limit");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn host_binding_reopens_absolute_and_parent_component_names() {
    use std::os::unix::fs::symlink;

    use qubit_local_files::policy::LocalSymlinkPolicy;

    let local = tempfile::tempdir().expect("local fixture should be created");
    let external = tempfile::tempdir().expect("external fixture should be created");
    std::fs::create_dir(local.path().join("sub")).expect("subdirectory should be created");
    std::fs::write(local.path().join("payload"), b"payload").expect("local payload should be created");
    std::fs::write(external.path().join("payload"), b"external").expect("external payload should be created");
    symlink(local.path().join("payload"), local.path().join("absolute-link"))
        .expect("absolute target link should be created");
    symlink(external.path().join("payload"), local.path().join("cross-scope-link"))
        .expect("cross-scope absolute target link should be created");

    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect("Host policy should follow links across scope");
    let options = LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new());
    let names = [
        local.path().join("absolute-link"),
        local.path().join("sub/../payload"),
        local.path().join("cross-scope-link"),
    ];
    for name in names {
        let reader = filesystem
            .open_observed_reader(&name, &options)
            .expect("Host reader should open the absolute name");
        assert_eq!(
            reader.verify_path_binding().expect("Host binding should verify"),
            LocalSourceVerification::Unchanged,
            "Host binding should preserve the original absolute name: {name:?}",
        );
    }
}

#[cfg(unix)]
#[test]
fn test_virtual_absolute_link_detects_replacement_despite_parent_hardlink() {
    let scratch = tempfile::tempdir().expect("fixture should exist");
    std::fs::create_dir(scratch.path().join("sub")).expect("sub should exist");
    std::fs::write(scratch.path().join("payload"), b"old").expect("old bytes");
    std::fs::hard_link(scratch.path().join("payload"), scratch.path().join("sub/payload"))
        .expect("old identity alias should exist");
    std::os::unix::fs::symlink("/payload", scratch.path().join("sub/link"))
        .expect("virtual absolute link should exist");
    let filesystem = LocalFileSystem::rooted(scratch.path()).expect("root should open");
    let reader = filesystem
        .open_observed_reader(
            std::path::Path::new("sub/link"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader should open");
    std::fs::write(scratch.path().join("replacement"), b"new").expect("new bytes");
    std::fs::rename(scratch.path().join("replacement"), scratch.path().join("payload"))
        .expect("original namespace target should be replaced");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Changed,
    );
}

#[cfg(unix)]
#[test]
fn binding_reports_unchanged_for_relative_and_virtual_absolute_links() {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("sub")).expect("subdirectory should be created");
    std::fs::write(tmp.path().join("payload"), b"root payload").expect("root payload should be created");
    std::fs::write(tmp.path().join("sub/payload"), b"local payload").expect("local payload should be created");
    std::os::unix::fs::symlink("payload", tmp.path().join("sub/local-link"))
        .expect("same-directory symlink should be created");
    std::os::unix::fs::symlink("../payload", tmp.path().join("sub/parent-link"))
        .expect("parent-relative symlink should be created");
    std::os::unix::fs::symlink("/payload", tmp.path().join("sub/absolute-link"))
        .expect("virtual absolute symlink should be created");
    let mut filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted policy should follow in-scope links");

    for path in ["sub/local-link", "sub/parent-link", "sub/absolute-link"] {
        let reader = filesystem
            .open_observed_reader(
                std::path::Path::new(path),
                &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
            )
            .expect("reader should open through the link");
        assert_eq!(
            reader.verify_path_binding().expect("binding should verify"),
            LocalSourceVerification::Unchanged,
            "binding should retain the original link namespace: {path}",
        );
    }
}

#[cfg(windows)]
#[test]
fn rooted_binding_verifies_native_file_symlink() {
    use qubit_local_files::policy::LocalSymlinkPolicy;

    let root = tempfile::tempdir().expect("temporary root should be created");
    std::fs::write(root.path().join("payload"), b"payload").expect("payload should be created");
    if !create_file_link(std::path::Path::new("payload"), &root.path().join("link")) {
        return;
    }
    let mut filesystem = LocalFileSystem::rooted(root.path()).expect("root authority should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted policy should follow in-scope links");
    let reader = filesystem
        .open_observed_reader(
            std::path::Path::new("link"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader should open through native link");

    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Unchanged,
    );
}

#[cfg(unix)]
#[test]
fn binding_reopens_relative_name_after_working_directory_changes() {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("first")).expect("first directory should be created");
    std::fs::create_dir(tmp.path().join("second")).expect("second directory should be created");
    std::fs::write(tmp.path().join("first/source"), b"source").expect("source should be created");
    std::fs::write(tmp.path().join("second/source"), b"source").expect("second source should be created");
    let mut filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    filesystem
        .set_current_directory(std::path::Path::new("first"))
        .expect("first pwd should set");
    let reader = filesystem
        .open_observed_reader(
            std::path::Path::new("source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader should open");
    filesystem
        .set_current_directory(std::path::Path::new("/second"))
        .expect("second pwd should set");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Unchanged,
    );
}

#[cfg(unix)]
#[test]
fn binding_preserves_parent_components_and_reports_missing_target() {
    let tmp = tempfile::tempdir().expect("temporary directory should be created");
    std::fs::create_dir(tmp.path().join("dir")).expect("directory should be created");
    std::fs::write(tmp.path().join("source"), b"source").expect("source should be created");
    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let reader = filesystem
        .open_observed_reader(
            std::path::Path::new("dir/../source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader should open");
    assert_eq!(
        reader.verify_path_binding().expect("binding should verify"),
        LocalSourceVerification::Unchanged,
    );
    std::fs::remove_file(tmp.path().join("source")).expect("source should be removed");
    assert_eq!(
        reader.verify_path_binding().expect("missing target should be reported"),
        LocalSourceVerification::Missing,
    );
}

#[cfg(unix)]
#[test]
fn rooted_directory_binding_reopens_the_original_symlink_recipe() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("fixture directory should exist");
    std::fs::create_dir(tmp.path().join("real")).expect("target directory should exist");
    std::fs::create_dir(tmp.path().join("other")).expect("replacement directory should exist");
    std::fs::write(tmp.path().join("real/source"), b"source").expect("source should exist");
    std::fs::write(tmp.path().join("other/source"), b"replacement").expect("replacement should exist");
    symlink("real", tmp.path().join("alias")).expect("directory alias should exist");

    let filesystem = LocalFileSystem::rooted(tmp.path()).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(
            std::path::Path::new("alias"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound alias directory should open");
    let reader = directory
        .open_observed_reader(std::path::Path::new("source"), &LocalObservedReadOptions::new())
        .expect("reader should open");

    std::fs::remove_file(tmp.path().join("alias")).expect("old alias should be removed");
    symlink("other", tmp.path().join("alias")).expect("replacement alias should be installed");

    assert_eq!(
        reader.verify_path_binding().expect("binding should be rechecked"),
        LocalSourceVerification::Changed,
    );
}
