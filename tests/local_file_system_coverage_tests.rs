// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{fs, io::Read, time::Duration};

use qubit_local_files::{
    LocalAtomicityRequirement, LocalCopyOptions, LocalDurabilityRequirement, LocalFileErrorKind,
    LocalFileSystem, LocalListOptions, LocalMetadataPreservePolicy, LocalReadOptions,
    LocalRenameOptions, LocalSymlinkPolicy, LocalWriteMode, LocalWriteOptions,
};
use tempfile::tempdir;

/// Verifies copy rejects directory guarantees that the recursive native
/// pipeline cannot publish.
#[test]
fn test_copy_directory_rejects_required_publication_guarantees() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    fs::create_dir(&source).expect("source directory should be created");

    for options in [
        LocalCopyOptions::new()
            .with_recursive()
            .with_atomicity(LocalAtomicityRequirement::Required),
        LocalCopyOptions::new()
            .with_recursive()
            .with_durability(LocalDurabilityRequirement::Required),
    ] {
        let error = LocalFileSystem::copy(&source, &directory.path().join("target"), &options)
            .expect_err("unsupported directory guarantee must fail before copy");
        assert_eq!(LocalFileErrorKind::RequirementNotMet, error.error().kind());
    }
}

/// Verifies append writer setup rejects required atomicity and non-file
/// destinations before opening a native stream.
#[test]
fn test_open_writer_append_rejects_unsupported_atomicity_and_directory() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("payload");
    fs::write(&file, b"payload").expect("file fixture should be written");

    let atomicity_error = LocalFileSystem::open_writer(
        &file,
        &LocalWriteOptions::new(LocalWriteMode::Append)
            .with_atomicity(LocalAtomicityRequirement::Required),
    )
    .expect_err("direct append cannot provide required atomicity");
    assert_eq!(
        LocalFileErrorKind::RequirementNotMet,
        atomicity_error.kind()
    );

    let type_error = LocalFileSystem::open_writer(
        directory.path(),
        &LocalWriteOptions::new(LocalWriteMode::Append),
    )
    .expect_err("directories cannot be opened for direct append");
    assert_eq!(LocalFileErrorKind::TypeConflict, type_error.kind());
}

/// Verifies a final symbolic-link source is rejected by default and may be
/// copied through only with the explicit follow policy.
#[cfg(unix)]
#[test]
fn test_copy_symlink_requires_explicit_follow_policy() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let link = directory.path().join("link");
    let target = directory.path().join("target");
    fs::write(&referent, b"payload").expect("referent should be written");
    symlink(&referent, &link).expect("file symlink should be created");

    let rejection = LocalFileSystem::copy(&link, &target, &LocalCopyOptions::new())
        .expect_err("default copy policy must reject a source symlink");
    assert_eq!(LocalFileErrorKind::Unsupported, rejection.error().kind());

    let outcome = LocalFileSystem::copy(
        &link,
        &target,
        &LocalCopyOptions::new().with_symlink_policy(LocalSymlinkPolicy::Follow),
    )
    .expect("follow policy should copy the referent file");
    assert!(outcome.atomic());
    assert_eq!(
        b"payload",
        fs::read(target)
            .expect("followed target should be readable")
            .as_slice(),
    );
}

/// Verifies host facade success paths exercise configured retry policies and
/// traversal through their public entry points.
#[test]
fn test_host_facade_uses_configured_reader_writer_and_list_policies() {
    let directory = tempdir().expect("temporary directory should be created");
    let _capabilities = LocalFileSystem::capabilities();
    let file = directory.path().join("payload");
    fs::write(&file, b"payload").expect("file fixture should be written");

    let mut reader = LocalFileSystem::open_reader(
        &file,
        &LocalReadOptions::new().with_open_retry_timeout(Duration::ZERO),
    )
    .expect("regular file should open with an explicit retry timeout");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("reader should return the fixture bytes");
    assert_eq!("payload", content);

    let mut writer = LocalFileSystem::open_writer(
        &file,
        &LocalWriteOptions::new(LocalWriteMode::Append).with_open_retry_timeout(Duration::ZERO),
    )
    .expect("append writer should open with an explicit retry timeout");
    use std::io::Write;
    writer
        .write_all(b"-appended")
        .expect("append writer should accept bytes");
    let outcome = writer.commit().expect("append writer should commit");
    assert!(!outcome.atomic());

    let walker = LocalFileSystem::list(directory.path(), &LocalListOptions::new())
        .expect("directory walker should open");
    let entries = walker
        .collect::<Result<Vec<_>, _>>()
        .expect("directory walker should yield its regular-file entry");
    assert_eq!(1, entries.len());
    assert_eq!(
        b"payload-appended",
        fs::read(&file)
            .expect("appended fixture should be readable")
            .as_slice(),
    );
}

/// Verifies copy and rename retain structured unchanged failures for missing
/// sources and textual aliases.
#[test]
fn test_copy_and_rename_reject_missing_sources_and_aliases() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing = directory.path().join("missing");
    let target = directory.path().join("target");

    let copy_error = LocalFileSystem::copy(&missing, &target, &LocalCopyOptions::new())
        .expect_err("missing copy source must fail");
    assert_eq!(LocalFileErrorKind::NotFound, copy_error.error().kind());

    fs::write(&target, b"payload").expect("alias fixture should be written");
    let alias_error = LocalFileSystem::copy(&target, &target, &LocalCopyOptions::new())
        .expect_err("copying a path onto itself must fail");
    assert_eq!(LocalFileErrorKind::InvalidInput, alias_error.error().kind());

    let rename_error = LocalFileSystem::rename(
        &missing,
        &directory.path().join("renamed"),
        &LocalRenameOptions::new(),
    )
    .expect_err("missing rename source must fail");
    assert_eq!(LocalFileErrorKind::NotFound, rename_error.error().kind());
}

/// Verifies directory and file operations exercise their successful mutation
/// paths, including metadata preservation policy conversion.
#[test]
fn test_host_facade_mutates_file_and_directory_entries() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"payload").expect("source fixture should be written");

    let copy = LocalFileSystem::copy(
        &source,
        &target,
        &LocalCopyOptions::new()
            .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
    )
    .expect("file copy should preserve the selected metadata policy");
    assert!(copy.atomic());
    assert_eq!(
        LocalMetadataPreservePolicy::Permissions,
        copy.metadata_preservation(),
    );

    let renamed = directory.path().join("renamed");
    let rename = LocalFileSystem::rename(&target, &renamed, &LocalRenameOptions::new())
        .expect("file rename should succeed when the target is absent");
    assert!(rename.atomic());

    let tree = directory.path().join("tree");
    let created = LocalFileSystem::create_directory(
        &tree,
        &qubit_local_files::LocalCreateDirectoryOptions::new(),
    )
    .expect("directory should be created");
    assert!(created.created());
    let deleted_directory =
        LocalFileSystem::delete_directory(&tree, &qubit_local_files::LocalDeleteOptions::new())
            .expect("empty directory should be deleted");
    assert!(deleted_directory.deleted());
    let deleted_file =
        LocalFileSystem::delete_file(&renamed, &qubit_local_files::LocalDeleteOptions::new())
            .expect("regular file should be deleted");
    assert!(deleted_file.deleted());
}

/// Runs one coverage-only facade fault in an isolated child process.
#[cfg(coverage)]
fn run_facade_fault<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if let Some(selected) = std::env::var_os(COVERAGE_FAULT_ENV) {
        if selected == std::ffi::OsStr::new(fault) {
            action();
        }
        return;
    }
    let executable = std::env::current_exe().expect("coverage test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child should launch");
    assert!(status.success(), "coverage fault child should pass");
}

/// Verifies an injected native rename uncertainty retains its indeterminate
/// public failure state.
#[cfg(coverage)]
#[test]
fn test_rename_reports_injected_indeterminate_native_failure() {
    const TEST_NAME: &str = "test_rename_reports_injected_indeterminate_native_failure";
    run_facade_fault(TEST_NAME, "rename-native-indeterminate", || {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source fixture should be written");

        let error = LocalFileSystem::rename(&source, &target, &LocalRenameOptions::new())
            .expect_err("injected native uncertainty must fail rename");
        assert_eq!(
            qubit_local_files::LocalRenameFailureState::Indeterminate,
            error.state(),
        );
    });
}

/// Verifies an I/O failure reported by the native rename boundary preserves
/// the indeterminate mutation state.
#[cfg(coverage)]
#[test]
fn test_rename_reports_injected_native_boundary_failure() {
    const TEST_NAME: &str = "test_rename_reports_injected_native_boundary_failure";
    run_facade_fault(TEST_NAME, "local-fs-rename-native-error", || {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source fixture should be written");

        let error = LocalFileSystem::rename(&source, &target, &LocalRenameOptions::new())
            .expect_err("injected native rename failure must be reported");
        assert_eq!(
            qubit_local_files::LocalRenameFailureState::Indeterminate,
            error.state()
        );
        assert!(source.exists(), "injected pre-native failure keeps source");
        assert!(
            !target.exists(),
            "injected pre-native failure keeps target absent"
        );
    });
}

/// Verifies host facade I/O conversions preserve structured failures at each
/// native filesystem boundary.
#[cfg(coverage)]
#[test]
fn test_host_facade_reports_injected_native_io_failures() {
    const TEST_NAME: &str = "test_host_facade_reports_injected_native_io_failures";
    for fault in [
        "local-fs-open-reader-metadata",
        "local-fs-open-writer-parent",
        "local-fs-copy-source-metadata",
        "local-fs-create-directory-exists",
        "local-fs-delete-file-remove",
        "local-fs-delete-directory-remove",
        "local-fs-delete-metadata",
        "local-fs-rename-source-metadata",
        "local-fs-open-reader-native",
        "local-fs-open-writer-append-metadata",
        "local-fs-open-writer-append-native",
        "local-fs-copy-follow-metadata",
        "local-fs-copy-target-metadata",
    ] {
        run_facade_fault(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            fs::write(&source, b"payload").expect("source fixture should be written");

            let failed = match fault {
                "local-fs-open-reader-metadata" => {
                    LocalFileSystem::open_reader(&source, &LocalReadOptions::new()).is_err()
                }
                "local-fs-open-writer-parent" => LocalFileSystem::open_writer(
                    &directory.path().join("nested/target"),
                    &LocalWriteOptions::new(LocalWriteMode::CreateNew).with_parent(),
                )
                .is_err(),
                "local-fs-copy-source-metadata" => {
                    LocalFileSystem::copy(&source, &target, &LocalCopyOptions::new()).is_err()
                }
                "local-fs-create-directory-exists" => LocalFileSystem::create_directory(
                    &target,
                    &qubit_local_files::LocalCreateDirectoryOptions::new(),
                )
                .is_err(),
                "local-fs-delete-file-remove" => LocalFileSystem::delete_file(
                    &source,
                    &qubit_local_files::LocalDeleteOptions::new(),
                )
                .is_err(),
                "local-fs-delete-directory-remove" => {
                    fs::create_dir(&target).expect("directory deletion fixture should be created");
                    LocalFileSystem::delete_directory(
                        &target,
                        &qubit_local_files::LocalDeleteOptions::new(),
                    )
                    .is_err()
                }
                "local-fs-delete-metadata" => LocalFileSystem::delete_file(
                    &source,
                    &qubit_local_files::LocalDeleteOptions::new(),
                )
                .is_err(),
                "local-fs-rename-source-metadata" => {
                    LocalFileSystem::rename(&source, &target, &LocalRenameOptions::new()).is_err()
                }
                "local-fs-open-reader-native" => {
                    LocalFileSystem::open_reader(&source, &LocalReadOptions::new()).is_err()
                }
                "local-fs-open-writer-append-metadata" | "local-fs-open-writer-append-native" => {
                    LocalFileSystem::open_writer(
                        &source,
                        &LocalWriteOptions::new(LocalWriteMode::Append),
                    )
                    .is_err()
                }
                "local-fs-copy-follow-metadata" => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::symlink;

                        let link = directory.path().join("link");
                        symlink(&source, &link).expect("source symlink should be created");
                        LocalFileSystem::copy(
                            &link,
                            &target,
                            &LocalCopyOptions::new()
                                .with_symlink_policy(LocalSymlinkPolicy::Follow),
                        )
                        .is_err()
                    }
                    #[cfg(not(unix))]
                    {
                        unreachable!("copy follow metadata fault requires Unix symlinks")
                    }
                }
                "local-fs-copy-target-metadata" => {
                    fs::write(&target, b"existing").expect("target fixture should be written");
                    LocalFileSystem::copy(&source, &target, &LocalCopyOptions::new()).is_err()
                }
                _ => unreachable!("every requested coverage fault is handled"),
            };
            assert!(failed, "injected {fault} must fail the facade");
        });
    }
}

/// Verifies copy and rename report post-publication durability failures from
/// the shared parent-sync native boundary.
#[cfg(coverage)]
#[test]
fn test_copy_and_rename_report_injected_parent_sync_failures() {
    const TEST_NAME: &str = "test_copy_and_rename_report_injected_parent_sync_failures";
    for fault in ["copy-parent-sync", "rename-parent-sync"] {
        run_facade_fault(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            fs::write(&source, b"payload").expect("source fixture should be written");

            let failed = match fault {
                "copy-parent-sync" => LocalFileSystem::copy(
                    &source,
                    &target,
                    &LocalCopyOptions::new().with_durability(LocalDurabilityRequirement::Required),
                )
                .is_err(),
                "rename-parent-sync" => LocalFileSystem::rename(
                    &source,
                    &target,
                    &LocalRenameOptions::new()
                        .with_durability(LocalDurabilityRequirement::Required),
                )
                .is_err(),
                _ => unreachable!("every parent-sync fault is handled"),
            };
            assert!(failed, "injected {fault} must fail publication durability");
            assert!(
                target.exists(),
                "native publication must precede parent sync"
            );
        });
    }
}

/// Verifies a host lacking directory synchronization rejects a required
/// durability request before the copy publishes its target.
#[cfg(coverage)]
#[test]
fn test_copy_rejects_injected_missing_directory_durability() {
    const TEST_NAME: &str = "test_copy_rejects_injected_missing_directory_durability";
    run_facade_fault(TEST_NAME, "local-fs-required-directory-durability", || {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source fixture should be written");

        let error = LocalFileSystem::copy(
            &source,
            &target,
            &LocalCopyOptions::new().with_durability(LocalDurabilityRequirement::Required),
        )
        .expect_err("required durability must fail when unavailable");
        assert_eq!(LocalFileErrorKind::RequirementNotMet, error.error().kind());
        assert!(!target.exists(), "preflight must not publish a target");
    });
}
