// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(feature = "digest")]

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::capability::LocalCapabilitySupport;
use qubit_local_files::capability::LocalConditionalKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
#[cfg(windows)]
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::policy::LocalFileSyncRequirement;
use qubit_local_files::policy::LocalSymlinkPolicy;
use sha2::Digest;
use sha2::Sha256;

fn fixture_root(fixture: &tempfile::TempDir) -> PathBuf {
    fs::canonicalize(fixture.path()).expect("canonicalize fixture directory")
}

#[test]
fn test_lf3_conditional_absent_fixed_parent() {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let root = fixture_root(&fixture);
    let original_parent = root.join("parent");
    fs::create_dir(&original_parent).expect("create parent");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let parent = fsys
        .open_directory_with_options(&original_parent, &LocalDirectoryOpenOptions::new())
        .expect("open fixed parent");
    let renamed_parent = root.join("renamed-parent");
    fs::rename(&original_parent, &renamed_parent).expect("rename parent");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew).with_file_sync(LocalFileSyncRequirement::Required);
    assert_eq!(
        parent.conditional_support(LocalConditionalKind::Absent, &options),
        LocalCapabilitySupport::CanAttempt
    );
    let mut writer = parent
        .open_conditional_writer::<Sha256>(Path::new("new"), LocalWritePrecondition::Absent, &options)
        .expect("open conditional writer");
    writer.write_fully(b"verified").expect("write candidate");
    let outcome = writer.commit().expect("publish candidate");
    assert!(outcome.write_outcome().file_synced());
    assert_eq!(
        fs::read(renamed_parent.join("new")).expect("read installed object"),
        b"verified"
    );
}

#[test]
fn test_lf3_conditional_fixed_parent_absent_conflict_keeps_target() {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let root = fixture_root(&fixture);
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let parent = fsys
        .open_directory_with_options(&root, &LocalDirectoryOpenOptions::new())
        .expect("open fixed parent");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
    let mut writer = parent
        .open_conditional_writer::<Sha256>(Path::new("target"), LocalWritePrecondition::Absent, &options)
        .expect("open conditional writer");
    writer.write_fully(b"candidate").expect("write candidate");
    fs::write(root.join("target"), b"competitor").expect("install competitor");
    let error = writer.commit().expect_err("late target must conflict");
    assert_eq!(
        error.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(fs::read(root.join("target")).expect("read competitor"), b"competitor");
    assert!(error.candidate().is_some(), "candidate remains recoverable");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn test_lf3_conditional_fixed_parent_content_conflict_keeps_original() {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let root = fixture_root(&fixture);
    fs::write(root.join("target"), b"current").expect("write target");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let parent = fsys
        .open_directory_with_options(&root, &LocalDirectoryOpenOptions::new())
        .expect("open fixed parent");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = parent
        .open_conditional_writer::<Sha256>(
            Path::new("target"),
            LocalWritePrecondition::Content(Sha256::digest(b"stale")),
            &options,
        )
        .expect("open conditional writer");
    writer.write_fully(b"candidate").expect("write candidate");
    let error = writer.commit().expect_err("stale digest must conflict");
    assert_eq!(
        error.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(
        error.reason(),
        qubit_local_files::error::LocalConditionalFailureReason::PreconditionMismatch
    );
    assert_eq!(fs::read(root.join("target")).expect("read original"), b"current");
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
#[test]
fn test_lf3_conditional_content_is_rejected_before_path_work_on_unsupported_unix() {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let root = fixture_root(&fixture);
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let parent = fsys
        .open_directory_with_options(&root, &LocalDirectoryOpenOptions::new())
        .expect("open fixed parent");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    assert_eq!(
        parent.conditional_support(LocalConditionalKind::Content, &options),
        LocalCapabilitySupport::Unsupported
    );
    assert_eq!(
        parent.conditional_support(
            LocalConditionalKind::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        ),
        LocalCapabilitySupport::CanAttempt
    );

    let invalid_options = LocalWriteOptions::new(LocalWriteMode::Append);
    let error = parent
        .open_conditional_writer::<Sha256>(
            Path::new("bad\0path"),
            LocalWritePrecondition::Content(Sha256::digest(b"before")),
            &invalid_options,
        )
        .expect_err("invalid options retain priority");
    assert_eq!(
        error.kind(),
        qubit_local_files::error::LocalFileErrorKind::InvalidOptions
    );

    let error = parent
        .open_conditional_writer::<Sha256>(
            Path::new("bad\0path"),
            LocalWritePrecondition::Content(Sha256::digest(b"before")),
            &options,
        )
        .expect_err("unsupported protocol is rejected before path parsing");
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::Unsupported);
    assert_eq!(fs::read_dir(&root).expect("enumerate unchanged parent").count(), 0);
}

#[cfg(windows)]
#[allow(clippy::useless_conversion)]
#[test]
fn test_lf3_conditional_windows_content_preflight_and_rooted_absent_are_unchanged() {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let root = fixture_root(&fixture);
    fs::write(root.join("target"), b"before").expect("write target");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let parent = fsys
        .open_directory_with_options(&root, &LocalDirectoryOpenOptions::new())
        .expect("open fixed parent");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let before = fs::read_dir(&root)
        .expect("enumerate before")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        parent.conditional_support(LocalConditionalKind::Content, &options),
        LocalCapabilitySupport::Unsupported
    );
    let error = parent
        .open_conditional_writer::<Sha256>(
            Path::new("target"),
            LocalWritePrecondition::Content(Sha256::digest(b"before").into()),
            &options,
        )
        .expect_err("rooted Content is unsupported");
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::Unsupported);
    let after = fs::read_dir(&root)
        .expect("enumerate after")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(before, after);

    let rooted = LocalFileSystem::rooted(&root).expect("open rooted filesystem");
    let mut rooted = rooted;
    rooted
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set rooted link policy");
    let writer = rooted
        .open_conditional_writer::<Sha256>(Path::new("direct-new"), LocalWritePrecondition::Absent, &options)
        .expect("Rooted Absent remains supported");
    drop(writer);
    let final_entries = fs::read_dir(&root)
        .expect("enumerate after direct Rooted open")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(before, final_entries);
}

#[cfg(windows)]
#[test]
fn test_lf3_conditional_windows_rooted_required_durability_preflight_is_unchanged() {
    let fixture = tempfile::tempdir().expect("fixture directory");
    let root = fixture_root(&fixture);
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let parent = fsys
        .open_directory_with_options(&root, &LocalDirectoryOpenOptions::new())
        .expect("open fixed parent");
    let options =
        LocalWriteOptions::new(LocalWriteMode::CreateNew).with_durability(LocalDurabilityRequirement::Required);
    let before = fs::read_dir(&root)
        .expect("enumerate before")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        parent.conditional_support(LocalConditionalKind::Absent, &options),
        LocalCapabilitySupport::Unsupported
    );
    let error = parent
        .open_conditional_writer::<Sha256>(Path::new("new"), LocalWritePrecondition::Absent, &options)
        .expect_err("Rooted Required durability is unsupported");
    assert_eq!(
        error.kind(),
        qubit_local_files::error::LocalFileErrorKind::RequirementNotMet
    );
    let after = fs::read_dir(&root)
        .expect("enumerate after")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(before, after);
}

#[cfg(windows)]
#[allow(clippy::useless_conversion)]
#[test]
fn test_lf3_conditional_windows_host_content_syncs_installed_object() {
    use sha2::Digest;

    let fixture = tempfile::tempdir().expect("fixture directory");
    let target = fixture_root(&fixture).join("target");
    fs::write(&target, b"previous").expect("write previous content");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_metadata_policy(qubit_local_files::options::LocalWriteMetadataPolicy::PreserveExisting);
    let mut writer = fsys
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"previous").into()),
            &options,
        )
        .expect("open conditional writer");
    writer.write_fully(b"replacement").expect("write candidate");
    let outcome = writer.commit().expect("publish replacement");
    assert!(outcome.write_outcome().file_synced());
    assert_eq!(fs::read(&target).expect("read installed target"), b"replacement");
}
