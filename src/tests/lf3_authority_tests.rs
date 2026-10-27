// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use qubit_io::Output;

use crate::directory::LocalComponentPolicy;
use crate::error::LocalFileErrorKind;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;

#[cfg(unix)]
fn fixture_on_canonical_current_directory() -> tempfile::TempDir {
    let current = std::env::current_dir().expect("current directory should be available");
    let current = std::fs::canonicalize(current).expect("current directory should be canonical");
    tempfile::tempdir_in(current).expect("fixture should exist")
}

#[test]
fn lf3_authority_rejects_absolute_and_escape() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let handle = fs
        .open_directory_with_options(std::path::Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("directory should open");
    assert_eq!(
        handle
            .entry_metadata(std::path::Path::new("../outside"))
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::InvalidPath
    );
    assert_eq!(
        handle.entry_metadata(fixture.path()).unwrap_err().kind(),
        LocalFileErrorKind::InvalidPath
    );
    assert!(LocalComponentPolicy::deny_names(vec!["..".into()]).is_err());
    assert!(LocalComponentPolicy::deny_names(vec!["".into()]).is_err());
    assert!(LocalComponentPolicy::deny_names(vec!["a/b".into()]).is_err());
    std::fs::create_dir(fixture.path().join("present")).expect("present directory should exist");
    assert_eq!(
        handle
            .entry_metadata(std::path::Path::new("missing/.."))
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::NotFound
    );
    std::fs::create_dir(fixture.path().join("allowed")).expect("allowed directory should exist");
    let restricted = fs
        .open_directory_with_options(
            std::path::Path::new("/"),
            &LocalDirectoryOpenOptions::new()
                .with_component_policy(LocalComponentPolicy::deny_names(vec!["denied".into()]).unwrap()),
        )
        .expect("restricted directory should open");
    assert_eq!(
        restricted
            .entry_metadata(std::path::Path::new("allowed/../denied"))
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::InvalidPath
    );
}

#[cfg(unix)]
#[test]
fn lf3_authority_checks_expanded_symlink_components() {
    let fixture = fixture_on_canonical_current_directory();
    let denied = fixture.path().join("private");
    std::fs::create_dir(&denied).expect("denied directory should exist");
    std::fs::write(denied.join("value"), b"value").expect("target file should exist");
    std::os::unix::fs::symlink("private/value", fixture.path().join("alias")).expect("link should exist");
    let fs = crate::LocalFileSystem::host().expect("host should open");
    let policy = LocalComponentPolicy::deny_names(vec!["private".into()]).expect("policy should be valid");
    let handle = fs
        .open_directory_with_options(
            fixture.path(),
            &LocalDirectoryOpenOptions::new().with_component_policy(policy),
        )
        .expect("directory should open");
    assert_eq!(
        handle
            .open_observed_reader(std::path::Path::new("alias"), &LocalObservedReadOptions::new())
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::InvalidPath
    );
}

#[cfg(unix)]
#[test]
fn lf3_authority_checks_symlink_components_before_parent_collapse() {
    let fixture = fixture_on_canonical_current_directory();
    std::fs::create_dir(fixture.path().join("private")).expect("denied directory should exist");
    std::fs::create_dir(fixture.path().join("public")).expect("target directory should exist");
    std::fs::write(fixture.path().join("public/value"), b"value").expect("target should exist");
    std::os::unix::fs::symlink("private/../public/value", fixture.path().join("alias")).expect("link should exist");
    let fs = crate::LocalFileSystem::host().expect("host should open");
    let policy = LocalComponentPolicy::deny_names(vec!["private".into()]).expect("policy should be valid");
    let handle = fs
        .open_directory_with_options(
            fixture.path(),
            &LocalDirectoryOpenOptions::new().with_component_policy(policy),
        )
        .expect("directory should open");
    assert_eq!(
        handle
            .open_observed_reader(std::path::Path::new("alias"), &LocalObservedReadOptions::new())
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::InvalidPath,
    );
}

#[cfg(unix)]
#[test]
fn lf3_authority_initial_host_open_checks_link_target_policy() {
    let fixture = fixture_on_canonical_current_directory();
    let denied = fixture.path().join("private");
    std::fs::create_dir(&denied).expect("target directory should exist");
    let alias = fixture.path().join("alias");
    std::os::unix::fs::symlink(&denied, &alias).expect("directory link should exist");
    let fs = crate::LocalFileSystem::host().expect("host should open");
    let policy = LocalComponentPolicy::deny_names(vec!["private".into()]).expect("policy should be valid");
    assert_eq!(
        fs.open_directory_with_options(&alias, &LocalDirectoryOpenOptions::new().with_component_policy(policy),)
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::InvalidPath,
    );
    assert_eq!(
        fs.open_directory_with_options(
            &alias,
            &LocalDirectoryOpenOptions::new().with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject),
        )
        .unwrap_err()
        .kind(),
        LocalFileErrorKind::Unsupported,
    );
}

#[cfg(not(unix))]
#[test]
fn lf3_authority_windows_host_rejects_unverifiable_component_policy() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = crate::LocalFileSystem::host().expect("host should open");
    let policy = LocalComponentPolicy::deny_names(vec!["blocked".into()]).unwrap();
    assert_eq!(
        fs.open_directory_with_options(
            fixture.path(),
            &LocalDirectoryOpenOptions::new().with_component_policy(policy),
        )
        .unwrap_err()
        .kind(),
        LocalFileErrorKind::Unsupported,
    );
}

#[cfg(windows)]
#[test]
fn lf3_authority_windows_host_rejects_unavailable_path_binding() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    std::fs::write(&source, b"source").expect("source should exist");
    let fs = crate::LocalFileSystem::host().expect("host should open");
    let binding = LocalPathBindingOptions::new();
    assert_eq!(
        fs.open_directory_with_options(
            fixture.path(),
            &LocalDirectoryOpenOptions::new().with_path_binding(binding),
        )
        .unwrap_err()
        .kind(),
        LocalFileErrorKind::Unsupported,
    );
    assert_eq!(
        fs.open_observed_reader(&source, &LocalObservedReadOptions::new().with_path_binding(binding),)
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::Unsupported,
    );
}

#[test]
fn lf3_authority_applies_component_policy_to_facades() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let policy = LocalComponentPolicy::deny_names(vec!["blocked".into()]).expect("policy should be valid");
    let handle = fs
        .open_directory_with_options(
            std::path::Path::new("/"),
            &LocalDirectoryOpenOptions::new().with_component_policy(policy),
        )
        .expect("directory should open");
    assert_eq!(
        handle
            .list_with_options(std::path::Path::new("blocked"), &crate::LocalListOptions::new())
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::InvalidPath,
    );
    assert_eq!(
        handle
            .create_directory_with_options(
                std::path::Path::new("blocked"),
                &crate::LocalCreateDirectoryOptions::new()
            )
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::InvalidPath,
    );
    let failure = handle
        .copy_with_options(
            std::path::Path::new("blocked/source"),
            std::path::Path::new("target"),
            &crate::LocalCopyOptions::new(),
        )
        .unwrap_err();
    assert_eq!(failure.error().kind(), LocalFileErrorKind::InvalidPath);
}

#[test]
fn lf3_authority_survives_directory_rename_and_replacement() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let directory = fixture.path().join("d");
    let old = fixture.path().join("old");
    std::fs::create_dir(&directory).expect("directory should exist");
    let fs = crate::LocalFileSystem::host().expect("host should open");
    let handle = fs
        .open_directory_with_options(&directory, &LocalDirectoryOpenOptions::new())
        .expect("authority should open");
    std::fs::rename(&directory, &old).expect("original directory should move");
    std::fs::create_dir(&directory).expect("replacement should exist");
    let mut writer = handle
        .open_writer_with_options(
            std::path::Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("writer should use retained directory");
    writer.write_fully(b"original").expect("write should succeed");
    let _outcome = writer.commit().expect("commit should succeed");
    assert_eq!(
        std::fs::read(old.join("payload")).expect("old target should exist"),
        b"original"
    );
    assert!(!directory.join("payload").exists());
}

#[test]
fn lf3_authority_opens_and_retains_rooted_child() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let directory = fixture.path().join("d");
    let old = fixture.path().join("old");
    std::fs::create_dir(&directory).expect("directory should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let handle = fs
        .open_directory_with_options(std::path::Path::new("/d"), &LocalDirectoryOpenOptions::new())
        .expect("rooted child should open");
    std::fs::rename(&directory, &old).expect("directory should move");
    std::fs::create_dir(&directory).expect("replacement should exist");
    let mut writer = handle
        .open_writer_with_options(
            std::path::Path::new("payload"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("writer should use retained child");
    writer.write_fully(b"rooted").expect("write should succeed");
    let _outcome = writer.commit().expect("commit should succeed");
    assert_eq!(
        std::fs::read(old.join("payload")).expect("old child should receive write"),
        b"rooted"
    );
    assert!(!directory.join("payload").exists());
}

#[test]
fn lf3_authority_binding_detects_final_name_replacement() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let path = fixture.path().join("source");
    std::fs::write(&path, b"source").expect("source should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let observed = fs
        .open_observed_reader(
            std::path::Path::new("/source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader and binding should open");
    assert_eq!(
        observed.verify_path_binding().unwrap(),
        crate::observation::LocalSourceVerification::Unchanged
    );
    std::fs::rename(&path, fixture.path().join("old")).expect("source should move");
    std::fs::write(&path, b"replacement").expect("replacement should exist");
    assert_eq!(
        observed.verify_path_binding().unwrap(),
        crate::observation::LocalSourceVerification::Changed
    );
}

#[test]
fn lf3_authority_binding_and_snapshot_keep_name_and_handle_observations_separate() {
    use crate::options::LocalSnapshotOptions;

    let fixture = tempfile::tempdir().expect("fixture should exist");
    let path = fixture.path().join("source");
    std::fs::write(&path, b"opened object").expect("source should exist");
    let filesystem = crate::LocalFileSystem::rooted(fixture.path()).expect("root should open");
    let snapshot = filesystem
        .read_consistent_with_options(
            std::path::Path::new("/source"),
            &LocalSnapshotOptions::builder(64).build(),
        )
        .expect("same-handle snapshot should be captured");
    assert_eq!(snapshot.bytes(), b"opened object");
    let mut reader = filesystem
        .open_observed_reader(
            std::path::Path::new("/source"),
            &LocalObservedReadOptions::new()
                .with_native_observation(crate::observation::LocalObservationRequirement::Required)
                .with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("observed reader and name binding should open");
    let opened_identity = reader
        .observation()
        .identity()
        .expect("native identity should be present")
        .clone();
    assert_eq!(snapshot.before().identity(), Some(&opened_identity));
    assert_eq!(snapshot.after().identity(), Some(&opened_identity));

    std::fs::rename(&path, fixture.path().join("renamed-source")).expect("move source name");
    std::fs::write(&path, b"replacement").expect("install replacement name");

    assert_eq!(
        reader.verify_path_binding().expect("verify name binding"),
        crate::observation::LocalSourceVerification::Changed
    );
    assert_eq!(
        reader.read_to_end_limited(64).expect("read retained handle"),
        b"opened object"
    );
    assert_eq!(
        reader.observe_now().expect("refresh opened handle").identity(),
        Some(&opened_identity)
    );
}

#[test]
fn lf3_authority_binding_reports_disappeared_final_name() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let path = fixture.path().join("source");
    std::fs::write(&path, b"source").expect("source should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let reader = fs
        .open_observed_reader(
            std::path::Path::new("/source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader should open");
    std::fs::remove_file(&path).expect("source should disappear");
    assert_eq!(
        reader.verify_path_binding().unwrap(),
        crate::observation::LocalSourceVerification::Missing
    );
}

#[test]
fn lf3_authority_binding_enforces_zero_budgets() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let path = fixture.path().join("source");
    std::fs::write(&path, b"source").expect("source should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    for binding in [
        LocalPathBindingOptions::new().with_max_ancestors(0),
        LocalPathBindingOptions::new().with_max_path_bytes(0),
    ] {
        assert_eq!(
            fs.open_observed_reader(
                std::path::Path::new("/source"),
                &LocalObservedReadOptions::new().with_path_binding(binding),
            )
            .unwrap_err()
            .kind(),
            LocalFileErrorKind::ResourceLimit,
        );
    }
    let error = fs
        .open_observed_reader(
            std::path::Path::new("/source"),
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(std::time::Duration::ZERO)),
        )
        .unwrap_err();
    assert_eq!(error.io_error().unwrap().kind(), std::io::ErrorKind::TimedOut);
}

#[test]
fn lf3_authority_binding_budgets_expanded_components_and_anchor_paths() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let nested = fixture.path().join("first").join("second");
    std::fs::create_dir_all(&nested).expect("nested directories should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    for maximum in [1, 8] {
        let result = fs.open_directory_with_options(
            std::path::Path::new("/first/second"),
            &LocalDirectoryOpenOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(maximum)),
        );
        assert_eq!(result.unwrap_err().kind(), LocalFileErrorKind::ResourceLimit);
    }
}

#[test]
fn lf3_authority_binding_uses_one_budget_for_resolution_and_anchors() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let child = fixture.path().join("child");
    std::fs::create_dir(&child).expect("child should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted fs should open");
    let anchor_bytes = fixture.path().as_os_str().len() + child.as_os_str().len();
    let result = fs.open_directory_with_options(
        std::path::Path::new("child"),
        &LocalDirectoryOpenOptions::new()
            .with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(anchor_bytes)),
    );
    assert_eq!(result.unwrap_err().kind(), LocalFileErrorKind::ResourceLimit);
}

#[test]
fn lf3_observed_reader_binding_shares_budget_across_parent_resolution_and_anchors() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let parent = fixture.path().join("parent");
    std::fs::create_dir(&parent).expect("parent should exist");
    std::fs::write(parent.join("source"), b"source").expect("source should exist");
    let filesystem = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted fs should open");
    let retained_anchor_bytes = fixture.path().as_os_str().len() + parent.as_os_str().len();
    let maximum = retained_anchor_bytes.max(42) + 1;
    let result = filesystem.open_observed_reader(
        std::path::Path::new("parent/source"),
        &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_path_bytes(maximum)),
    );
    assert_eq!(result.unwrap_err().kind(), LocalFileErrorKind::ResourceLimit);
}

#[cfg(unix)]
#[test]
fn lf3_authority_binding_detects_parent_replacement() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let parent = fixture.path().join("parent");
    let old = fixture.path().join("old-parent");
    std::fs::create_dir(&parent).expect("parent should exist");
    let path = parent.join("source");
    std::fs::write(&path, b"source").expect("source should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let observed = fs
        .open_observed_reader(
            std::path::Path::new("/parent/source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("reader and binding should open");
    std::fs::rename(&parent, &old).expect("parent should move");
    std::fs::create_dir(&parent).expect("replacement parent should exist");
    assert_eq!(
        observed.verify_path_binding().unwrap(),
        crate::observation::LocalSourceVerification::Changed
    );
}

#[cfg(unix)]
#[test]
fn lf3_authority_binding_detects_fixed_root_replacement() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let directory = fixture.path().join("d");
    let old = fixture.path().join("old");
    std::fs::create_dir(&directory).expect("directory should exist");
    std::fs::write(directory.join("source"), b"source").expect("source should exist");
    let fs = crate::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let handle = fs
        .open_directory_with_options(
            std::path::Path::new("/d"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound directory should open");
    assert!(
        !handle
            .ancestor_observations(&crate::LocalListOptions::new())
            .unwrap()
            .is_empty()
    );
    let reader = handle
        .open_observed_reader(std::path::Path::new("source"), &LocalObservedReadOptions::new())
        .expect("reader should open");
    std::fs::rename(&directory, &old).expect("directory should move");
    std::fs::create_dir(&directory).expect("replacement should exist");
    assert_eq!(
        reader.verify_path_binding().unwrap(),
        crate::observation::LocalSourceVerification::Changed
    );
}
