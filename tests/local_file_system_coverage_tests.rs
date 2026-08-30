// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(any(target_os = "linux", target_os = "android"))]
use std::ffi::CString;
use std::fs;
use std::io::Read;
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalAtomicityRequirement;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalCopyConflictPolicy;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalCopyFailureState;
use qubit_local_files::LocalCopyOptions;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalCopyTypeConflictPolicy;
use qubit_local_files::LocalCreateDirectoryOptions;
use qubit_local_files::LocalDeleteOptions;
use qubit_local_files::LocalDurabilityRequirement;
use qubit_local_files::LocalFileErrorKind;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalFileOperation;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalListOptions;
use qubit_local_files::LocalMetadataPreservePolicy;
use qubit_local_files::LocalReadOptions;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalRenameFailureState;
use qubit_local_files::LocalRenameOptions;
use qubit_local_files::LocalSymlinkPolicy;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalTempDirectoryOptions;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalTempFileOptions;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::install_test_fault;
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
            .with_tree_source()
            .with_atomicity(LocalAtomicityRequirement::Required),
        LocalCopyOptions::new()
            .with_tree_source()
            .with_durability(LocalDurabilityRequirement::Required),
    ] {
        let error = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&source, &directory.path().join("target"), &options)
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

    let atomicity_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_writer_with_options(
            &file,
            &LocalWriteOptions::new(LocalWriteMode::Append).with_atomicity(LocalAtomicityRequirement::Required),
        )
        .expect_err("direct append cannot provide required atomicity");
    assert_eq!(LocalFileErrorKind::RequirementNotMet, atomicity_error.kind());

    let type_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_writer_with_options(directory.path(), &LocalWriteOptions::new(LocalWriteMode::Append))
        .expect_err("directories cannot be opened for direct append");
    assert_eq!(LocalFileErrorKind::TypeConflict, type_error.kind());
}

/// Verifies a final symbolic-link source is copied as a link entry by default.
#[cfg(unix)]
#[test]
fn test_copy_symlink_preserves_final_link_entry() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let link = directory.path().join("link");
    let target = directory.path().join("target");
    fs::write(&referent, b"payload").expect("referent should be written");
    symlink(&referent, &link).expect("file symlink should be created");

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&link, &target, &LocalCopyOptions::new())
        .expect("default copy should copy a source link entry");
    assert!(!outcome.atomic());
    assert_eq!(referent, fs::read_link(&target).expect("target link should exist"));

    let target_follow = directory.path().join("target-follow");
    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(
            &link,
            &target_follow,
            &LocalCopyOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        )
        .expect("explicit follow should still preserve the final link entry");
    assert!(!outcome.atomic());
    assert_eq!(
        referent,
        fs::read_link(target_follow).expect("target link should exist")
    );
}

/// Verifies host facade success paths exercise configured retry policies and
/// traversal through their public entry points.
#[test]
fn test_host_facade_uses_configured_reader_writer_and_list_policies() {
    let directory = tempdir().expect("temporary directory should be created");
    let _capabilities = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .protocols();
    let file = directory.path().join("payload");
    fs::write(&file, b"payload").expect("file fixture should be written");

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&file, &LocalReadOptions::new().with_open_retry_timeout(Duration::ZERO))
        .expect("regular file should open with an explicit retry timeout");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("reader should return the fixture bytes");
    assert_eq!("payload", content);

    let mut writer = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_writer_with_options(
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

    let walker = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .list_with_options(directory.path(), &LocalListOptions::new())
        .expect("directory walker should open");
    let entries = walker
        .collect::<Result<Vec<_>, _>>()
        .expect("directory walker should yield its regular-file entry");
    assert_eq!(1, entries.len());
    assert_eq!(
        b"payload-appended",
        fs::read(&file).expect("appended fixture should be readable").as_slice(),
    );
}

/// Verifies copy and rename retain structured unchanged failures for missing
/// sources and textual aliases.
#[test]
fn test_copy_and_rename_reject_missing_sources_and_aliases() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing = directory.path().join("missing");
    let target = directory.path().join("target");

    let copy_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&missing, &target, &LocalCopyOptions::new())
        .expect_err("missing copy source must fail");
    assert_eq!(LocalFileErrorKind::NotFound, copy_error.error().kind());

    fs::write(&target, b"payload").expect("alias fixture should be written");
    let alias_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&target, &target, &LocalCopyOptions::new())
        .expect_err("copying a path onto itself must fail");
    assert_eq!(LocalFileErrorKind::InvalidOptions, alias_error.error().kind(),);

    let rename_error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(&missing, &directory.path().join("renamed"), &LocalRenameOptions::new())
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

    let copy = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new().with_metadata_preservation(LocalMetadataPreservePolicy::Permissions),
        )
        .expect("file copy should preserve the selected metadata policy");
    assert!(copy.atomic());
    assert_eq!(LocalMetadataPreservePolicy::Permissions, copy.metadata_preservation(),);

    let renamed = directory.path().join("renamed");
    let rename = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(&target, &renamed, &LocalRenameOptions::new())
        .expect("file rename should succeed when the target is absent");
    assert!(rename.atomic());

    let tree = directory.path().join("tree");
    let created = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&tree, &LocalCreateDirectoryOptions::new())
        .expect("directory should be created");
    assert!(created.created());
    let deleted_directory = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_directory_with_options(&tree, &LocalDeleteOptions::new())
        .expect("empty directory should be deleted");
    assert!(deleted_directory.deleted());
    let deleted_file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_file_with_options(&renamed, &LocalDeleteOptions::new())
        .expect("regular file should be deleted");
    assert!(deleted_file.deleted());
}

/// Verifies atomic replacement retains descriptor-visible user metadata on
/// Linux and Android filesystems that support extended attributes.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn test_atomic_replacement_preserves_extended_attributes() {
    use std::io::Write;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("destination");
    fs::write(&path, b"previous").expect("destination fixture should be written");
    let native_path = CString::new(path.as_os_str().as_bytes()).expect("temporary path should not contain NUL");
    let attribute = CString::new("user.qubit-local-files-coverage").expect("attribute name should not contain NUL");
    let value = b"preserved";

    let set_result = unsafe {
        libc::setxattr(
            native_path.as_ptr(),
            attribute.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if set_result == -1 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOTSUP) {
            return;
        }
        panic!("destination xattr should be created: {error}");
    }

    let mut writer = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_writer_with_options(&path, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
        .expect("existing destination should open for atomic replacement");
    writer
        .write_all(b"replacement")
        .expect("staging writer should accept replacement bytes");
    let outcome = writer.commit().expect("atomic replacement should commit");
    assert!(outcome.atomic());

    let mut observed = [0_u8; 16];
    let length = unsafe {
        libc::getxattr(
            native_path.as_ptr(),
            attribute.as_ptr(),
            observed.as_mut_ptr().cast(),
            observed.len(),
        )
    };
    assert_eq!(value.len() as isize, length);
    assert_eq!(value, &observed[..length as usize]);
}

/// Verifies namespace metadata probes and symlink-policy transitions preserve
/// the distinct host and rooted authority contracts.
#[test]
fn test_filesystem_namespace_capabilities_and_probe_variants() {
    let directory = tempdir().expect("temporary directory should be created");
    let mut host = LocalFileSystem::host().expect("Host filesystem should open");
    host.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("host policy changes should be accepted");
    assert_eq!(LocalSymlinkPolicy::Reject, host.symlink_policy(),);
    let _ = host.limits();
    let _ = host
        .limits_at(&directory.path().join("missing/leaf"))
        .expect("host limits should probe nearest existing ancestor");
    let _ = host
        .space_at(&directory.path().join("missing/leaf"))
        .expect("host space should probe nearest existing ancestor");

    let mut rooted = LocalFileSystem::rooted(directory.path()).expect("rooted authority should open");
    assert!(rooted.diagnostic_root().is_some());
    let _ = rooted.limits();
    let _ = rooted
        .limits_at(Path::new("missing/leaf"))
        .expect("rooted limits should probe nearest existing ancestor");
    let _ = rooted
        .space_at(Path::new("missing/leaf"))
        .expect("rooted space should probe nearest existing ancestor");
    assert!(
        rooted
            .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
            .is_err()
    );
}

/// Runs one test-support-only facade fault in an isolated child process.
#[cfg(feature = "internal-test-support")]
fn run_facade_fault<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_ENV).is_some_and(|selected| selected == std::ffi::OsStr::new(fault)) {
        let _fault = install_test_fault(fault).expect("test fault controller should install");
        action();
        return;
    }
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some() {
        return;
    }
    let executable = std::env::current_exe().expect("coverage test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(TEST_FAULT_ENV, fault)
        .env(TEST_FAULT_CHILD_ENV, "1")
        .status()
        .expect("test fault child should launch");
    assert!(status.success(), "test fault child should pass");
}

/// Verifies metadata-preserving atomic replacement reports every injected
/// native xattr and metadata boundary through the public writer API.
#[cfg(all(feature = "internal-test-support", any(target_os = "linux", target_os = "android")))]
#[test]
fn test_atomic_replacement_exercises_metadata_fault_boundaries() {
    const TEST_NAME: &str = "test_atomic_replacement_exercises_metadata_fault_boundaries";
    for (fault, fails) in [
        ("atomic-metadata-source-stat", true),
        ("atomic-metadata-staging-stat", true),
        ("atomic-metadata-owner", true),
        ("atomic-metadata-owner-native", true),
        ("atomic-metadata-mode", true),
        ("atomic-metadata-native-mode", true),
        ("atomic-metadata-not-supported", false),
        ("atomic-metadata-list", true),
        ("atomic-metadata-list-read", true),
        ("atomic-metadata-list-range", true),
        ("atomic-metadata-list-range-persistent", true),
        ("atomic-metadata-security-name", true),
        ("atomic-metadata-invalid-name", true),
        ("atomic-metadata-equal-value", false),
        ("atomic-metadata-source-missing", true),
        ("atomic-metadata-read", true),
        ("atomic-metadata-value-range-persistent", true),
        ("atomic-metadata-value-read", true),
        ("atomic-metadata-write", true),
        ("atomic-metadata-remove", true),
        ("atomic-metadata-staging-list", true),
    ] {
        run_facade_fault(TEST_NAME, fault, || {
            use std::io::Write;

            let directory = tempdir().expect("temporary directory should be created");
            let path = directory.path().join("destination");
            fs::write(&path, b"previous").expect("destination fixture should be written");
            let native_path = CString::new(path.as_os_str().as_bytes()).expect("temporary path should not contain NUL");
            let attribute =
                CString::new("user.qubit-local-files-fault").expect("attribute name should not contain NUL");
            let value = b"value";
            let set_result = unsafe {
                libc::setxattr(
                    native_path.as_ptr(),
                    attribute.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            };
            if set_result == -1 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ENOTSUP) {
                    return;
                }
                panic!("destination xattr should be created: {error}");
            }

            let mut writer = LocalFileSystem::host()
                .expect("Host filesystem should open")
                .open_writer_with_options(&path, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
                .expect("existing destination should open for replacement");
            writer
                .write_all(b"replacement")
                .expect("staging writer should accept replacement bytes");
            assert_eq!(
                fails,
                writer.commit().is_err(),
                "selected {fault} should have the documented outcome",
            );
        });
    }
}

/// Verifies a post-open prefix-read failure is attributed to the read stage.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_read_prefix_reports_injected_read_failure() {
    const TEST_NAME: &str = "test_read_prefix_reports_injected_read_failure";
    run_facade_fault(TEST_NAME, "local-fs-read-prefix-read", || {
        let directory = tempdir().expect("temporary directory should be created");
        let file = directory.path().join("payload");
        fs::write(&file, b"payload").expect("fixture should be written");

        let error = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .read_prefix_with_options(&file, 4, &LocalReadOptions::new())
            .expect_err("injected read failure must be reported");
        assert_eq!(LocalFileOperation::Read, error.operation());
        assert_eq!(Some(file.as_path()), error.path());
    });
}

/// Verifies an injected native rename uncertainty retains its indeterminate
/// public failure state.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_rename_reports_injected_indeterminate_native_failure() {
    const TEST_NAME: &str = "test_rename_reports_injected_indeterminate_native_failure";
    run_facade_fault(TEST_NAME, "rename-native-indeterminate", || {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source fixture should be written");

        let error = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .rename_with_options(&source, &target, &LocalRenameOptions::new())
            .expect_err("injected native uncertainty must fail rename");
        assert_eq!(LocalRenameFailureState::Indeterminate, error.state(),);
    });
}

/// Verifies an I/O failure reported by the native rename boundary preserves
/// the indeterminate mutation state.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_rename_reports_injected_native_boundary_failure() {
    const TEST_NAME: &str = "test_rename_reports_injected_native_boundary_failure";
    run_facade_fault(TEST_NAME, "local-fs-rename-native-error", || {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source fixture should be written");

        let error = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .rename_with_options(&source, &target, &LocalRenameOptions::new())
            .expect_err("injected native rename failure must be reported");
        assert_eq!(LocalRenameFailureState::Indeterminate, error.state());
        assert!(source.exists(), "injected pre-native failure keeps source");
        assert!(!target.exists(), "injected pre-native failure keeps target absent");
    });
}

/// Verifies host facade I/O conversions preserve structured failures at each
/// native filesystem boundary.
#[cfg(feature = "internal-test-support")]
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
        "local-fs-copy-target-metadata",
    ] {
        run_facade_fault(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let source = directory.path().join("source");
            let target = directory.path().join("target");
            fs::write(&source, b"payload").expect("source fixture should be written");

            let failed = match fault {
                "local-fs-open-reader-metadata" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .open_reader_with_options(&source, &LocalReadOptions::new())
                    .is_err(),
                "local-fs-open-writer-parent" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .open_writer_with_options(
                        &directory.path().join("nested/target"),
                        &LocalWriteOptions::new(LocalWriteMode::CreateNew).with_parent(),
                    )
                    .is_err(),
                "local-fs-copy-source-metadata" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .copy_with_options(&source, &target, &LocalCopyOptions::new())
                    .is_err(),
                "local-fs-create-directory-exists" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .create_directory_with_options(&target, &LocalCreateDirectoryOptions::new())
                    .is_err(),
                "local-fs-delete-file-remove" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .delete_file_with_options(&source, &LocalDeleteOptions::new())
                    .is_err(),
                "local-fs-delete-directory-remove" => {
                    fs::create_dir(&target).expect("directory deletion fixture should be created");
                    LocalFileSystem::host()
                        .expect("Host filesystem should open")
                        .delete_directory_with_options(&target, &LocalDeleteOptions::new())
                        .is_err()
                }
                "local-fs-delete-metadata" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .delete_file_with_options(&source, &LocalDeleteOptions::new())
                    .is_err(),
                "local-fs-rename-source-metadata" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .rename_with_options(&source, &target, &LocalRenameOptions::new())
                    .is_err(),
                "local-fs-open-reader-native" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .open_reader_with_options(&source, &LocalReadOptions::new())
                    .is_err(),
                "local-fs-open-writer-append-metadata" | "local-fs-open-writer-append-native" => {
                    LocalFileSystem::host()
                        .expect("Host filesystem should open")
                        .open_writer_with_options(&source, &LocalWriteOptions::new(LocalWriteMode::Append))
                        .is_err()
                }
                "local-fs-copy-target-metadata" => {
                    fs::write(&target, b"existing").expect("target fixture should be written");
                    LocalFileSystem::host()
                        .expect("Host filesystem should open")
                        .copy_with_options(&source, &target, &LocalCopyOptions::new())
                        .is_err()
                }
                _ => unreachable!("every requested test fault is handled"),
            };
            assert!(failed, "injected {fault} must fail the facade");
        });
    }
}

/// Verifies copy and rename report post-publication durability failures from
/// the shared parent-sync native boundary.
#[cfg(feature = "internal-test-support")]
#[cfg(not(windows))]
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
                "copy-parent-sync" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .copy_with_options(
                        &source,
                        &target,
                        &LocalCopyOptions::new().with_durability(LocalDurabilityRequirement::Required),
                    )
                    .is_err(),
                "rename-parent-sync" => LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .rename_with_options(
                        &source,
                        &target,
                        &LocalRenameOptions::new().with_durability(LocalDurabilityRequirement::Required),
                    )
                    .is_err(),
                _ => unreachable!("every parent-sync fault is handled"),
            };
            assert!(failed, "injected {fault} must fail publication durability");
            assert!(target.exists(), "native publication must precede parent sync");
        });
    }
}

/// Verifies required copy durability synchronizes the staging handle before
/// publication, leaving an existing destination unchanged on sync failure.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_required_durability_syncs_staging_before_publication() {
    const TEST_NAME: &str = "test_copy_required_durability_syncs_staging_before_publication";
    run_facade_fault(TEST_NAME, "copy-staging-file-sync", || {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"new").expect("source fixture should be written");
        fs::write(&target, b"old").expect("target fixture should be written");

        let failure = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::new()
                    .with_conflict(LocalCopyConflictPolicy::Overwrite)
                    .with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("staging synchronization failure must stop publication");

        assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
        assert_eq!(b"old", fs::read(&target).expect("target should remain").as_slice(),);
    });
}

/// Verifies a host lacking directory synchronization rejects a required
/// durability request before the copy publishes its target.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_copy_rejects_injected_missing_directory_durability() {
    const TEST_NAME: &str = "test_copy_rejects_injected_missing_directory_durability";
    run_facade_fault(TEST_NAME, "local-fs-required-directory-durability", || {
        let directory = tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"payload").expect("source fixture should be written");

        let error = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::new().with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("required durability must fail when unavailable");
        assert_eq!(LocalFileErrorKind::RequirementNotMet, error.error().kind());
        assert!(!target.exists(), "preflight must not publish a target");
    });
}

/// Verifies host recursive-copy recovery either reports a native destination
/// creation race or reconciles it into a complete tree publication.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_host_copy_reports_injected_destination_races() {
    const TEST_NAME: &str = "test_host_copy_reports_injected_destination_races";
    for fault in [
        "copy-directory-create-error",
        "copy-directory-race-existing",
        "copy-directory-race-inspect",
        "copy-directory-race-nondirectory",
    ] {
        run_facade_fault(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let source = directory.path().join("source");
            fs::create_dir_all(source.join("nested")).expect("source tree should be created");
            fs::write(source.join("nested/payload"), b"payload").expect("source payload should be written");

            let target = directory.path().join("target");
            let result = LocalFileSystem::host()
                .expect("Host filesystem should open")
                .copy_with_options(&source, &target, &LocalCopyOptions::new().with_tree_source());
            if result.is_ok() {
                assert_eq!(
                    b"payload",
                    fs::read(target.join("nested/payload"))
                        .expect("a reconciled successful copy must publish the full tree")
                        .as_slice(),
                );
            }
        });
    }
}

/// Verifies host recursive-copy replacement detects reinspection and removal
/// races before it can claim a completed tree publication.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_host_copy_reports_injected_destination_removal_races() {
    const TEST_NAME: &str = "test_host_copy_reports_injected_destination_removal_races";
    for fault in [
        "copy-removal-race-directory",
        "copy-removal-race-inspect",
        "copy-removal-race-not-found",
    ] {
        run_facade_fault(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let source = directory.path().join("source");
            fs::create_dir(&source).expect("source directory should be created");
            fs::write(source.join("payload"), b"payload").expect("source payload should be written");
            let target = directory.path().join("target");
            fs::write(&target, b"conflicting file").expect("conflicting target should be written");

            assert!(
                LocalFileSystem::host()
                    .expect("Host filesystem should open")
                    .copy_with_options(
                        &source,
                        &target,
                        &LocalCopyOptions::new()
                            .with_tree_source()
                            .with_conflict(LocalCopyConflictPolicy::Overwrite)
                            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
                    )
                    .is_err(),
                "injected destination removal race {fault} must fail the copy",
            );
        });
    }
}

/// Verifies host temporary-resource creation retries a one-shot collision and
/// returns native creation failures without creating cleanup-owned resources.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_host_temp_resources_report_injected_creation_outcomes() {
    const TEST_NAME: &str = "test_host_temp_resources_report_injected_creation_outcomes";
    for (fault, directory_resource, succeeds) in [
        ("temp-file-collision", false, true),
        ("temp-file-open", false, false),
        ("temp-directory-collision", true, true),
        ("temp-directory-create", true, false),
    ] {
        run_facade_fault(TEST_NAME, fault, || {
            let directory = tempdir().expect("temporary directory should be created");
            let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
            if directory_resource {
                let result = filesystem.create_temp_directory_with_options(
                    &LocalTempDirectoryOptions::new()
                        .with_parent(directory.path())
                        .with_max_attempts(2),
                );
                if succeeds {
                    let mut temporary = result.expect("a one-shot collision should be retried");
                    temporary.cleanup().expect("temporary directory should clean up");
                } else {
                    assert!(result.is_err(), "native directory creation fault must fail");
                }
            } else {
                let result = filesystem.create_temp_file_with_options(
                    &LocalTempFileOptions::new()
                        .with_parent(directory.path())
                        .with_max_attempts(2),
                );
                if succeeds {
                    let mut temporary = result.expect("a one-shot collision should be retried");
                    temporary.cleanup().expect("temporary file should clean up");
                } else {
                    assert!(result.is_err(), "native file creation fault must fail");
                }
            }
        });
    }
}
