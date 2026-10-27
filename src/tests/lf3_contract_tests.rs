// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::options::LocalSealOptions;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalFileSyncRequirement;
use crate::policy::LocalPrivacyRequirement;

#[test]
fn lf3_contract_defaults_preserve_old_guarantees() {
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
    assert_eq!(options.file_sync(), LocalFileSyncRequirement::NotRequired);
    assert_eq!(options.durability(), LocalDurabilityRequirement::NotRequired);
    assert_eq!(LocalSealOptions::new().file_sync(), LocalFileSyncRequirement::Required);
}

#[test]
fn lf3_contract_byte_traits_accept_unsized_values() {
    fn accepts_input<R: qubit_io::ByteInput + ?Sized>(_reader: &mut R) {}
    fn accepts_output<W: qubit_io::ByteOutput + ?Sized>(_writer: &mut W) {}

    let input: &mut dyn qubit_io::ByteInput = &mut std::io::Cursor::new(Vec::<u8>::new());
    let output: &mut dyn qubit_io::ByteOutput = &mut Vec::<u8>::new();
    accepts_input(input);
    accepts_output(output);
}

#[test]
fn lf3_contract_privacy_defaults_and_copy_configuration() {
    let file_options = crate::options::LocalTempFileOptions::new();
    let directory_options = crate::options::LocalTempDirectoryOptions::new();
    assert_eq!(file_options.privacy(), LocalPrivacyRequirement::PlatformDefault);
    assert_eq!(directory_options.privacy(), LocalPrivacyRequirement::PlatformDefault);
    assert_eq!(
        file_options
            .clone()
            .with_privacy(LocalPrivacyRequirement::Required)
            .privacy(),
        LocalPrivacyRequirement::Required
    );
    assert_eq!(file_options.privacy(), LocalPrivacyRequirement::PlatformDefault);
}

#[test]
fn lf3_contract_read_options_keep_explicit_budgets_and_snapshot_limit() {
    use crate::options::LocalObservedReadOptions;
    use crate::options::LocalPathBindingOptions;
    use crate::options::LocalReadAllOptions;
    use crate::options::LocalReadOptions;
    use crate::options::LocalSnapshotOptions;
    use crate::policy::LocalSymlinkPolicy;

    assert_eq!(LocalReadAllOptions::new().max_bytes(), None);
    assert_eq!(LocalReadAllOptions::new().read_options(), None);
    assert_eq!(
        LocalReadOptions::new()
            .with_symlink_policy(LocalSymlinkPolicy::Reject)
            .symlink_policy(),
        Some(LocalSymlinkPolicy::Reject)
    );
    let all = LocalReadAllOptions::new()
        .with_max_bytes(8)
        .with_read_options(LocalReadOptions::new())
        .with_symlink_policy(LocalSymlinkPolicy::Reject);
    assert_eq!(all.max_bytes(), Some(8));
    assert_eq!(all.symlink_policy(), Some(LocalSymlinkPolicy::Reject));
    assert_eq!(
        LocalObservedReadOptions::new().native_observation(),
        crate::observation::LocalObservationRequirement::Optional
    );
    assert_eq!(LocalPathBindingOptions::new().max_ancestors(), None);
    let snapshot = LocalSnapshotOptions::builder(0)
        .read_options(LocalReadOptions::new())
        .build();
    assert_eq!(snapshot.max_bytes(), 0);
}

#[test]
fn lf3_contract_invalid_encoding_retains_utf8_source() {
    use crate::error::LocalFileError;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileOperation;

    let invalid = String::from_utf8(vec![0xff]).expect_err("test byte is invalid UTF-8");
    let error = LocalFileError::from_utf8(LocalFileOperation::Read, None, invalid);
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidEncoding);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::InvalidData);
    assert!(matches!(
        error.typed_source(),
        Some(crate::error::LocalFileErrorSource::InvalidEncoding(_))
    ));
}

#[test]
fn lf3_contract_configuration_edit_uses_utf8_snapshot_and_fixed_writer() {
    use std::path::Path;

    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalSnapshotOptions;
    use crate::options::LocalWriteOptions;

    let fixture = tempfile::tempdir().expect("fixture");
    std::fs::write(fixture.path().join("settings.toml"), "title = \"原始\"\n").expect("initial config");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let text = filesystem
        .read_text(&fixture.path().join("settings.toml"))
        .expect("read UTF-8 config");
    assert_eq!(text, "title = \"原始\"\n");
    let snapshot = filesystem
        .read_consistent_with_options(
            &fixture.path().join("settings.toml"),
            &LocalSnapshotOptions::builder(128).build(),
        )
        .expect("same-handle snapshot");
    assert_eq!(snapshot.bytes(), text.as_bytes());

    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("fixed config directory");
    let mut writer = parent
        .open_writer_with_options(
            Path::new("settings.toml"),
            &LocalWriteOptions::new(crate::LocalWriteMode::CreateOrReplace),
        )
        .expect("fixed-directory writer");
    qubit_io::Output::write_fully(&mut writer, "title = \"更新\"\n".as_bytes()).expect("write updated config");
    let _outcome = writer.commit().expect("commit updated config");
    assert_eq!(
        filesystem
            .read_text(&fixture.path().join("settings.toml"))
            .expect("read updated config"),
        "title = \"更新\"\n"
    );
}

#[test]
fn lf3_contract_shared_cache_uses_private_sealed_native_file() {
    use std::io::Write;

    use crate::LocalFileSystem;
    use crate::LocalSealOptions;
    use crate::options::LocalTempFileOptions;
    use crate::policy::LocalPrivacyRequirement;

    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut cache = filesystem
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private cache file");
    cache
        .as_file_mut()
        .expect("writable cache handle")
        .write_all(b"cache payload")
        .expect("write cache data");
    let sealed = cache.seal_with_options(&LocalSealOptions::new()).expect("seal cache");
    assert_eq!(sealed.native_path(), Some(sealed.path()));
    assert_eq!(
        std::fs::read(sealed.native_path().expect("host path")).expect("cache bytes"),
        b"cache payload"
    );
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(sealed.native_path().expect("host path"))
            .is_err()
    );
    let mut sealed = sealed;
    sealed.cleanup().expect("cleanup cache");
}

#[cfg(unix)]
#[test]
fn lf3_contract_directory_backup_preserves_links_until_strictly_rejected() {
    use std::path::PathBuf;

    use crate::LocalFileKind;
    use crate::LocalFileSystem;
    use crate::directory::LocalTreeEntry;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTreeValidationOptions;

    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("backup parent");

    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("backup stage");
    let stage_path = fixture.path().join(stage.relative_name());
    std::fs::write(stage_path.join("payload"), b"inside tree").expect("backup payload");
    std::os::unix::fs::symlink("payload", stage_path.join("link")).expect("create preserved link");
    let entries = vec![
        LocalTreeEntry::new(PathBuf::from("payload"), LocalFileKind::File).expect("safe expected payload"),
        LocalTreeEntry::new(PathBuf::from("link"), LocalFileKind::Symlink).expect("safe expected link"),
    ];
    stage
        .verify_entries(&entries, &LocalTreeValidationOptions::new())
        .expect("default verification preserves legal links");
    let published = stage
        .publish_new(
            &parent,
            std::ffi::OsStr::new("backup"),
            &crate::options::LocalTreePublishOptions::new(),
        )
        .expect("backup should publish without following the link");
    assert_eq!(published.publication(), crate::outcome::LocalTreePublication::Published);
    assert!(
        std::fs::symlink_metadata(fixture.path().join("backup/link"))
            .expect("published link should remain")
            .file_type()
            .is_symlink()
    );

    let strict = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("strict stage");
    let strict_path = fixture.path().join(strict.relative_name());
    std::fs::write(strict_path.join("payload"), b"inside tree").expect("strict payload");
    std::os::unix::fs::symlink("payload", strict_path.join("link")).expect("create link for strict validation");
    let error = strict
        .verify_entries(&entries, &LocalTreeValidationOptions::new().with_reject_symlinks())
        .expect_err("explicit strict rule rejects the link");
    assert_eq!(error.kind(), crate::error::LocalFileErrorKind::RequirementNotMet);
    assert!(strict_path.join("link").exists(), "stage remains owned by caller");
    strict.cleanup().expect("cleanup rejected stage");
}

#[cfg(unix)]
#[test]
fn lf3_contract_component_policy_does_not_isolate_copy_from_ancestor_replacement() {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Barrier;

    use crate::LocalCopyConflictPolicy;
    use crate::LocalFileSystem;
    use crate::directory::LocalComponentPolicy;
    use crate::directory::install_component_policy_checkpoint;
    use crate::options::LocalCopyOptions;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::policy::LocalSymlinkPolicy;

    let fixture = tempfile::tempdir().expect("fixture");
    let allowed = fixture.path().join("allowed");
    let blocked = fixture.path().join("blocked");
    std::fs::create_dir(&allowed).expect("allowed directory");
    std::fs::create_dir(&blocked).expect("blocked directory");
    std::fs::write(allowed.join("source"), b"allowed source").expect("allowed source");
    std::fs::write(allowed.join("target"), b"allowed target").expect("allowed target");
    std::fs::write(blocked.join("source"), b"blocked source").expect("blocked source");
    std::fs::write(blocked.join("target"), b"blocked target").expect("blocked target");

    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem");
    let handle = filesystem
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
                .with_component_policy(LocalComponentPolicy::deny_names(vec!["blocked".into()]).expect("policy")),
        )
        .expect("fixed directory handle");
    let barrier = Arc::new(Barrier::new(2));
    install_component_policy_checkpoint(
        &handle,
        Path::new("allowed/source"),
        Path::new("allowed/target"),
        Arc::clone(&barrier),
    );

    let worker = std::thread::spawn(move || {
        handle
            .copy_with_options(
                Path::new("allowed/source"),
                Path::new("allowed/target"),
                &LocalCopyOptions::new().with_conflict(LocalCopyConflictPolicy::Overwrite),
            )
            .expect("copy through the replaced ancestor should complete")
    });
    barrier.wait();
    std::fs::rename(&allowed, fixture.path().join("allowed-original")).expect("move validated ancestor");
    std::os::unix::fs::symlink("blocked", &allowed).expect("replace ancestor with a link");
    barrier.wait();

    let _result = worker.join().expect("copy worker should complete");
    assert_eq!(
        b"blocked source",
        std::fs::read(blocked.join("target"))
            .expect("the replacement destination should exist")
            .as_slice()
    );
    assert_eq!(
        b"allowed target",
        std::fs::read(fixture.path().join("allowed-original/target"))
            .expect("the renamed original target should remain")
            .as_slice()
    );
}
