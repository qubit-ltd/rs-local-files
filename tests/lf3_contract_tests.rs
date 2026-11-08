// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io::Write;
use std::path::Path;

use qubit_io::ByteInput;
use qubit_io::ByteOutput;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileErrorSource;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalSealOptions;
use qubit_local_files::options::LocalSnapshotOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::policy::LocalFileSyncRequirement;
use qubit_local_files::policy::LocalPrivacyRequirement;

#[test]
fn test_lf3_contract_defaults_preserve_old_guarantees() {
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
    assert_eq!(options.file_sync(), LocalFileSyncRequirement::NotRequired);
    assert_eq!(options.durability(), LocalDurabilityRequirement::NotRequired);
    assert_eq!(LocalSealOptions::new().file_sync(), LocalFileSyncRequirement::Required);
}

#[test]
fn test_lf3_contract_byte_traits_accept_unsized_values() {
    fn accepts_input<R: ByteInput + ?Sized>(_reader: &mut R) {}
    fn accepts_output<W: ByteOutput + ?Sized>(_writer: &mut W) {}

    let input: &mut dyn ByteInput = &mut std::io::Cursor::new(Vec::<u8>::new());
    let output: &mut dyn ByteOutput = &mut Vec::<u8>::new();
    accepts_input(input);
    accepts_output(output);
}

#[test]
fn test_lf3_contract_privacy_defaults_and_copy_configuration() {
    let file_options = LocalTempFileOptions::new();
    let directory_options = LocalTempDirectoryOptions::new();
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
fn test_lf3_contract_invalid_encoding_retains_utf8_source() {
    let fixture = tempfile::tempdir().expect("fixture");
    let path = fixture.path().join("invalid.txt");
    std::fs::write(&path, [0xff]).expect("invalid UTF-8 bytes should be written");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let error = filesystem.read_text(&path).expect_err("invalid UTF-8 should fail");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidEncoding);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::InvalidData);
    assert!(matches!(
        error.typed_source(),
        Some(LocalFileErrorSource::InvalidEncoding(_))
    ));
}

#[test]
fn test_lf3_contract_configuration_edit_uses_utf8_snapshot_and_fixed_writer() {
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
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("fixed-directory writer");
    Output::write_fully(&mut writer, "title = \"更新\"\n".as_bytes()).expect("write updated config");
    let _outcome = writer.commit().expect("commit updated config");
    assert_eq!(
        filesystem
            .read_text(&fixture.path().join("settings.toml"))
            .expect("read updated config"),
        "title = \"更新\"\n"
    );
}

#[test]
fn test_lf3_contract_shared_cache_uses_private_sealed_native_file() {
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
