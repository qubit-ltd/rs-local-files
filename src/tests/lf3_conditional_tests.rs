// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use qubit_io::Output;
use sha2::Digest;
use sha2::Sha256;

use crate::LocalFileSystem;
use crate::LocalWriteMode;
use crate::LocalWriteOptions;
use crate::capability::LocalCapabilitySupport;
use crate::capability::LocalConditionalKind;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalWritePrecondition;
#[cfg(windows)]
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalFileSyncRequirement;
use crate::policy::LocalSymlinkPolicy;

fn fixture_root(fixture: &tempfile::TempDir) -> PathBuf {
    fs::canonicalize(fixture.path()).expect("canonicalize fixture directory")
}

#[test]
fn lf3_conditional_absent_fixed_parent() {
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
fn lf3_conditional_fixed_parent_absent_conflict_keeps_target() {
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
        crate::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(fs::read(root.join("target")).expect("read competitor"), b"competitor");
    assert!(error.candidate().is_some(), "candidate remains recoverable");
}

#[test]
#[cfg(unix)]
fn lf3_conditional_fixed_parent_content_conflict_keeps_original() {
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
        crate::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(
        error.reason(),
        crate::error::LocalConditionalFailureReason::PreconditionMismatch
    );
    assert_eq!(fs::read(root.join("target")).expect("read original"), b"current");
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_conditional_required_file_sync_failure_prevents_publication() {
    use crate::test_support::TestConditionalHooks;
    use crate::test_support::TestConditionalPoint;

    let fixture = tempfile::tempdir().expect("fixture directory");
    let target = fixture_root(&fixture).join("target");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let fsys = fsys.with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeRequiredFileSync,
        || Err(std::io::Error::other("injected sync failure")),
    ));
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew).with_file_sync(LocalFileSyncRequirement::Required);
    let mut writer = fsys
        .open_conditional_writer::<Sha256>(&target, LocalWritePrecondition::Absent, &options)
        .expect("open conditional writer");
    writer.write_fully(b"candidate").expect("write candidate");
    let error = writer
        .commit()
        .expect_err("required sync failure must abort before publish");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert!(!target.exists(), "target must remain absent");
    assert!(error.candidate().is_some(), "recovery candidate is retained");
    assert!(
        std::error::Error::source(error.error()).is_some(),
        "native error source survives"
    );
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_conditional_preferred_file_sync_failure_reports_false_and_publishes() {
    use crate::test_support::TestConditionalHooks;
    use crate::test_support::TestConditionalPoint;

    let fixture = tempfile::tempdir().expect("fixture directory");
    let target = fixture_root(&fixture).join("target");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let fsys = fsys.with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeInstalledFileSync,
        || Err(std::io::Error::other("injected preferred sync failure")),
    ));
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew).with_file_sync(LocalFileSyncRequirement::Preferred);
    let mut writer = fsys
        .open_conditional_writer::<Sha256>(&target, LocalWritePrecondition::Absent, &options)
        .expect("open conditional writer");
    writer.write_fully(b"candidate").expect("write candidate");
    let outcome = writer.commit().expect("preferred sync may continue");
    assert!(!outcome.write_outcome().file_synced());
    assert_eq!(fs::read(target).expect("read installed target"), b"candidate");
}

#[cfg(windows)]
#[test]
fn lf3_conditional_windows_content_preflight_and_rooted_absent_are_unchanged() {
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
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::RequirementNotMet);
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
fn lf3_conditional_windows_rooted_required_durability_preflight_is_unchanged() {
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
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::RequirementNotMet);
    let after = fs::read_dir(&root)
        .expect("enumerate after")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(before, after);
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_conditional_installed_sync_failure_is_published() {
    use crate::test_support::TestConditionalHooks;
    use crate::test_support::TestConditionalPoint;

    let fixture = tempfile::tempdir().expect("fixture directory");
    let target = fixture_root(&fixture).join("target");
    fs::write(&target, b"previous").expect("write previous content");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let fsys = fsys.with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeInstalledFileSync,
        || Err(std::io::Error::other("injected installed sync failure")),
    ));
    let options =
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_file_sync(LocalFileSyncRequirement::Required);
    let mut writer = fsys
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"previous")),
            &options,
        )
        .expect("open conditional writer");
    writer.write_fully(b"replacement").expect("write candidate");
    let error = writer.commit().expect_err("final installed sync is required");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::Published
    );
    assert_eq!(fs::read(&target).expect("read installed target"), b"replacement");
    assert!(
        std::error::Error::source(error.error()).is_some(),
        "native sync error is preserved"
    );
    assert!(!error.recovery_files().is_empty());
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_conditional_replaced_installed_entry_is_not_reported_synced() {
    use crate::test_support::TestConditionalHooks;
    use crate::test_support::TestConditionalPoint;

    let fixture = tempfile::tempdir().expect("fixture directory");
    let root = fixture_root(&fixture);
    let target = root.join("target");
    let replaced = root.join("replaced");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let hook_target = target.clone();
    let hook_replaced = replaced.clone();
    let fsys = fsys.with_conditional_test_hooks(TestConditionalHooks::once(
        TestConditionalPoint::BeforeInstalledFileSync,
        move || {
            fs::rename(&hook_target, &hook_replaced)?;
            fs::write(&hook_target, b"replacement by competitor")?;
            Ok(())
        },
    ));
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew).with_file_sync(LocalFileSyncRequirement::Required);
    let mut writer = fsys
        .open_conditional_writer::<Sha256>(&target, LocalWritePrecondition::Absent, &options)
        .expect("open conditional writer");
    writer.write_fully(b"candidate").expect("write candidate");
    let error = writer.commit().expect_err("replacement must fail final installed sync");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::Published
    );
    assert_eq!(
        fs::read(&target).expect("read competing replacement"),
        b"replacement by competitor"
    );
    assert_eq!(
        fs::read(&replaced).expect("read original installed object"),
        b"candidate"
    );
    assert!(
        std::error::Error::source(error.error()).is_some(),
        "identity mismatch remains the error source"
    );
}

#[cfg(windows)]
#[test]
fn lf3_conditional_windows_host_content_syncs_installed_object() {
    use sha2::Digest;

    let fixture = tempfile::tempdir().expect("fixture directory");
    let target = fixture_root(&fixture).join("target");
    fs::write(&target, b"previous").expect("write previous content");
    let mut fsys = LocalFileSystem::host().expect("host filesystem");
    fsys.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("set link policy");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_metadata_policy(crate::options::LocalWriteMetadataPolicy::PreserveExisting);
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
