// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    io::{
        Read,
        Write,
    },
    path::Path,
};

use qubit_local_files::{
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalFileErrorKind,
    LocalFileKind,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameFailureState,
    LocalRenameOptions,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
    RootedLocalFileSystem,
};

#[cfg(coverage)]
use qubit_local_files::{
    LocalCopyConflictPolicy,
    LocalCopyFailureState,
    LocalDurabilityRequirement,
};
use tempfile::tempdir;

/// Verifies default rooted copy and rename avoid durability synchronization.
#[cfg(target_os = "linux")]
#[test]
fn test_rooted_local_file_system_default_copy_and_rename_skip_sync() {
    const CHILD_ENV: &str = "QUBIT_LOCAL_FILES_DEFAULT_ROOTED_SYNC_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let directory = tempdir().expect("temporary root should be created");
        fs::write(directory.path().join("copy-source"), b"copy")
            .expect("copy source should be written");
        let rooted = RootedLocalFileSystem::open(directory.path())
            .expect("root authority should open");
        let _ = rooted
            .copy(
                Path::new("copy-source"),
                Path::new("copy-target"),
                &LocalCopyOptions::new(),
            )
            .expect("default rooted copy should succeed");

        fs::write(directory.path().join("rename-source"), b"rename")
            .expect("rename source should be written");
        let _ = rooted
            .rename(
                Path::new("rename-source"),
                Path::new("rename-target"),
                &LocalRenameOptions::new(),
            )
            .expect("default rooted rename should succeed");
        return;
    }

    if std::process::Command::new("strace")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!(
            "skipping default rooted sync trace because strace is unavailable"
        );
        return;
    }
    let trace =
        tempfile::NamedTempFile::new().expect("trace file should be created");
    let status = std::process::Command::new("strace")
        .args(["-f", "-e", "trace=fsync", "-o"])
        .arg(trace.path())
        .arg(std::env::current_exe().expect("test executable should resolve"))
        .args([
            "--exact",
            "test_rooted_local_file_system_default_copy_and_rename_skip_sync",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .status()
        .expect("strace should launch the traced child");
    assert!(status.success(), "traced child should succeed");
    let trace =
        fs::read_to_string(trace.path()).expect("trace should be readable");
    assert!(
        !trace.contains("fsync("),
        "default durability must not synchronize: {trace}"
    );
}

/// Verifies rooted copy can create missing destination parents on request.
#[test]
fn test_rooted_local_file_system_copy_creates_missing_parent() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload")
        .expect("source fixture should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let _ = rooted
        .copy(
            Path::new("source"),
            Path::new("nested/target"),
            &LocalCopyOptions::new().with_parent(),
        )
        .expect("rooted copy should create the missing parent");

    assert_eq!(
        b"payload",
        fs::read(directory.path().join("nested/target"))
            .expect("copied target should read")
            .as_slice()
    );
}

/// Verifies cleanup follows the root descriptor after its diagnostic path
/// moves.
#[test]
fn test_rooted_temp_file_cleanup_uses_retained_root_authority_after_root_rename()
 {
    let root_parent = tempdir().expect("root parent should exist");
    let original = root_parent.path().join("original-root");
    let renamed = root_parent.path().join("renamed-root");
    fs::create_dir(&original).expect("root should be created");
    let root = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");

    let mut temp = root
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("rooted temp file should be created");
    fs::rename(&original, &renamed)
        .expect("diagnostic root path should be renamed");
    temp.cleanup()
        .expect("cleanup must use retained root authority");

    assert!(!renamed.join(temp.path()).exists());
}

/// Verifies cleanup never follows a replacement at the diagnostic root path.
#[test]
fn test_rooted_temp_file_cleanup_ignores_replacement_diagnostic_root() {
    let root_parent = tempdir().expect("root parent should exist");
    let original = root_parent.path().join("original-root");
    let renamed = root_parent.path().join("renamed-root");
    fs::create_dir(&original).expect("root should be created");
    let root = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");
    let mut temp = root
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("rooted temp file should be created");
    let relative_path = temp.path().to_path_buf();

    fs::rename(&original, &renamed)
        .expect("diagnostic root path should be renamed");
    fs::create_dir(&original).expect("replacement root should be created");
    fs::write(original.join(&relative_path), b"replacement")
        .expect("replacement entry should be created");

    temp.cleanup()
        .expect("cleanup must use retained root authority");

    assert!(!renamed.join(&relative_path).exists());
    assert!(original.join(relative_path).exists());
}

/// Verifies directory cleanup follows retained root authority after
/// replacement.
#[test]
fn test_rooted_temp_directory_cleanup_uses_retained_root_authority_after_replacement()
 {
    let root_parent = tempdir().expect("root parent should exist");
    let original = root_parent.path().join("original-root");
    let renamed = root_parent.path().join("renamed-root");
    fs::create_dir(&original).expect("root should be created");
    let root = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");
    let mut temporary = root
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("rooted temp directory should be created");
    let relative_path = temporary.path().to_path_buf();

    fs::rename(&original, &renamed)
        .expect("diagnostic root path should be renamed");
    fs::create_dir(&original).expect("replacement root should be created");
    fs::create_dir(original.join(&relative_path))
        .expect("replacement entry should be created");
    temporary
        .cleanup()
        .expect("directory cleanup must use retained root authority");

    assert!(!renamed.join(&relative_path).exists());
    assert!(original.join(relative_path).exists());
}

/// Verifies a rooted persist conflict retains a cleanup-capable temporary file.
#[test]
fn test_rooted_temp_file_persist_conflict_retains_cleanup_responsibility() {
    let root_parent = tempdir().expect("root parent should exist");
    let root = RootedLocalFileSystem::open(root_parent.path())
        .expect("root authority should open");
    let temporary = root
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("rooted temp file should be created");
    let source = temporary.path().to_path_buf();
    let target = Path::new("existing-target");
    fs::write(root_parent.path().join(target), b"existing")
        .expect("target should exist");

    let mut error = temporary
        .persist(target)
        .expect_err("existing rooted target should reject persistence");

    assert_eq!(source, error.resource().path());
    error
        .resource_mut()
        .cleanup()
        .expect("retained resource should still clean up");
    assert!(!root_parent.path().join(source).exists());
}

/// Verifies dropping an owned rooted file cleans only the opened-root entry.
#[test]
fn test_rooted_temp_file_drop_uses_retained_authority_after_root_replacement() {
    let root_parent = tempdir().expect("root parent should exist");
    let original = root_parent.path().join("original-root");
    let renamed = root_parent.path().join("renamed-root");
    fs::create_dir(&original).expect("root should be created");
    let root = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");
    let relative_path = {
        let temporary = root
            .create_temp_file(&LocalTempFileOptions::new())
            .expect("rooted temp file should be created");
        let relative_path = temporary.path().to_path_buf();
        fs::rename(&original, &renamed)
            .expect("diagnostic root path should be renamed");
        fs::create_dir(&original).expect("replacement root should be created");
        fs::write(original.join(&relative_path), b"replacement")
            .expect("replacement entry should be created");
        relative_path
    };

    assert!(!renamed.join(&relative_path).exists());
    assert!(original.join(relative_path).exists());
}

/// Verifies a successful rooted persist disarms drop cleanup for the target.
#[test]
fn test_rooted_temp_file_persisted_target_survives_drop() {
    let root_parent = tempdir().expect("root parent should exist");
    let root = RootedLocalFileSystem::open(root_parent.path())
        .expect("root authority should open");
    let temporary = root
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("rooted temp file should be created");
    let target = Path::new("persisted-target");

    assert_eq!(
        target,
        temporary
            .persist(target)
            .expect("rooted file should persist")
    );
    assert!(root_parent.path().join(target).exists());
}

/// Verifies rooted persistence keeps using the opened root after diagnostic
/// rename.
#[test]
fn test_rooted_temp_file_persist_uses_retained_authority_after_root_rename() {
    let root_parent = tempdir().expect("root parent should exist");
    let original = root_parent.path().join("original-root");
    let renamed = root_parent.path().join("renamed-root");
    fs::create_dir(&original).expect("root should be created");
    let root = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");
    let temporary = root
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("rooted temp file should be created");

    fs::rename(&original, &renamed)
        .expect("diagnostic root path should be renamed");
    assert_eq!(
        Path::new("persisted-target"),
        temporary
            .persist(Path::new("persisted-target"))
            .expect("persist must use retained root authority")
    );
    assert!(renamed.join("persisted-target").exists());
}

/// Verifies an indeterminate native persist failure disables cleanup and drop
/// removal.
#[test]
fn test_rooted_temp_file_indeterminate_persist_failure_skips_cleanup_and_drop()
{
    let root_parent = tempdir().expect("root parent should exist");
    let root = RootedLocalFileSystem::open(root_parent.path())
        .expect("root authority should open");
    let temporary = root
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("rooted temp file should be created");
    let source = temporary.path().to_path_buf();

    let error = temporary
        .persist(Path::new("missing-parent/target"))
        .expect_err("missing rooted target parent should make publication indeterminate");
    let (_io, mut retained, _requested, _resolved, _stage) = error.into_parts();

    assert!(retained.cleanup().is_err());
    drop(retained);
    assert!(root_parent.path().join(&source).exists());
    fs::remove_file(root_parent.path().join(source))
        .expect("fixture should be removed manually");
}

/// Runs one coverage-only rooted-copy fault case in an isolated child process.
#[cfg(coverage)]
fn run_in_coverage_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if std::env::var_os(COVERAGE_FAULT_ENV).is_some() {
        action();
        return;
    }
    let executable =
        std::env::current_exe().expect("test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child should launch");
    assert!(status.success(), "coverage fault child should pass");
}

/// Verifies rooted recursive failures retain exact published-child statistics.
#[cfg(coverage)]
#[test]
fn test_rooted_copy_failure_reports_second_child_partial_publication() {
    const TEST_NAME: &str =
        "test_rooted_copy_failure_reports_second_child_partial_publication";
    run_in_coverage_fault_process(TEST_NAME, "rooted-copy-file-second", || {
        let directory =
            tempdir().expect("temporary directory should be created");
        let source = directory.path().join("source");
        fs::create_dir(&source).expect("source directory should be created");
        fs::write(source.join("first"), b"first")
            .expect("first child should be written");
        fs::write(source.join("second"), b"second")
            .expect("second child should be written");
        let rooted = RootedLocalFileSystem::open(directory.path())
            .expect("root authority should open");

        let failure = rooted
            .copy(
                Path::new("source"),
                Path::new("target"),
                &LocalCopyOptions::default().with_tree_source(),
            )
            .expect_err("second rooted child fault must fail");

        assert_eq!(
            qubit_local_files::LocalCopyFailureState::PartiallyPublished,
            failure.state()
        );
        assert_eq!(1, failure.partial_stats().files());
    });
}

/// Verifies rooted parent synchronization failures retain completed copy stats.
#[cfg(coverage)]
#[test]
fn test_rooted_copy_failure_retains_stats_after_parent_sync_fault() {
    const TEST_NAME: &str =
        "test_rooted_copy_failure_retains_stats_after_parent_sync_fault";
    run_in_coverage_fault_process(TEST_NAME, "rooted-copy-parent-sync", || {
        let directory =
            tempdir().expect("temporary directory should be created");
        fs::write(directory.path().join("source"), b"payload")
            .expect("source should be written");
        let rooted = RootedLocalFileSystem::open(directory.path())
            .expect("root authority should open");

        let failure = rooted
            .copy(
                Path::new("source"),
                Path::new("target"),
                &LocalCopyOptions::default()
                    .with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("rooted parent sync fault must fail");

        assert_eq!(LocalCopyFailureState::Published, failure.state());
        assert_eq!(1, failure.partial_stats().files());
    });
}

/// Verifies rooted commit cleanup failures retain root-relative staging
/// details.
#[cfg(coverage)]
#[test]
fn test_rooted_copy_failure_retains_cleanup_staging_context() {
    const TEST_NAME: &str =
        "test_rooted_copy_failure_retains_cleanup_staging_context";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-install-cleanup",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            fs::write(directory.path().join("source"), b"payload")
                .expect("source should be written");
            fs::write(directory.path().join("target"), b"existing")
                .expect("target should be written");
            let rooted = RootedLocalFileSystem::open(directory.path())
                .expect("root authority should open");

            let failure = rooted
                .copy(
                    Path::new("source"),
                    Path::new("target"),
                    &LocalCopyOptions::default()
                        .with_conflict(LocalCopyConflictPolicy::Overwrite),
                )
                .expect_err("rooted install and cleanup faults must fail");

            assert_eq!(LocalCopyFailureState::Indeterminate, failure.state());
            assert!(failure.staging_path().is_some());
            assert!(failure.cleanup_error().is_some());
            assert!(
                failure
                    .staging_path()
                    .expect("staging path should be retained")
                    .is_relative()
            );
        },
    );
}

/// Verifies a rooted parent durability fault retains the completed rename fact.
#[cfg(coverage)]
#[test]
fn test_rooted_rename_parent_durability_failure_reports_renamed() {
    const TEST_NAME: &str =
        "test_rooted_rename_parent_durability_failure_reports_renamed";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-rename-parent-sync",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            fs::write(directory.path().join("source"), b"payload")
                .expect("source should be written");
            let rooted = RootedLocalFileSystem::open(directory.path())
                .expect("root authority should open");

            let failure = rooted
                .rename(
                    Path::new("source"),
                    Path::new("target"),
                    &LocalRenameOptions::default()
                        .with_durability(LocalDurabilityRequirement::Required),
                )
                .expect_err("rooted parent durability fault must fail");

            assert_eq!(LocalRenameFailureState::Renamed, failure.state());
            assert!(!directory.path().join("source").exists());
            assert!(directory.path().join("target").exists());
        },
    );
}

/// Verifies an I/O failure at the rooted native boundary remains conservative.
#[cfg(coverage)]
#[test]
fn test_rooted_rename_native_io_failure_reports_indeterminate() {
    const TEST_NAME: &str =
        "test_rooted_rename_native_io_failure_reports_indeterminate";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-rename-indeterminate",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            fs::write(directory.path().join("source"), b"payload")
                .expect("source should be written");
            let rooted = RootedLocalFileSystem::open(directory.path())
                .expect("root authority should open");

            let failure = rooted
                .rename(
                    Path::new("source"),
                    Path::new("target"),
                    &LocalRenameOptions::default(),
                )
                .expect_err("rooted native I/O fault must fail");

            assert_eq!(LocalRenameFailureState::Indeterminate, failure.state());
        },
    );
}

/// Verifies a rooted native missing-source failure proves no rename occurred.
#[test]
fn test_rooted_rename_missing_source_reports_unchanged() {
    let directory = tempdir().expect("temporary directory should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let failure = rooted
        .rename(
            Path::new("missing-source"),
            Path::new("target"),
            &LocalRenameOptions::default(),
        )
        .expect_err("missing rooted source must fail");

    assert_eq!(LocalRenameFailureState::Unchanged, failure.state());
    assert!(!directory.path().join("target").exists());
}

/// Verifies rooted paths reject lexical escape components.
#[test]
fn test_rooted_local_file_system_rejects_lexical_escape() {
    let directory = tempdir().expect("temporary root should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let error = rooted
        .metadata(Path::new("../escape"))
        .expect_err("parent traversal must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidInput, error.kind());
}

/// Verifies rooted recursive listing remains within descriptor authority.
#[test]
fn test_rooted_local_file_system_lists_descendants() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("nested"))
        .expect("nested directory should be created");
    fs::write(directory.path().join("nested/child"), b"x")
        .expect("child should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let entries = rooted
        .list(
            Path::new("nested"),
            &LocalListOptions::new().with_recursive(),
        )
        .expect("rooted walker should be created")
        .collect::<Result<Vec<_>, _>>()
        .expect("rooted traversal should succeed");

    assert_eq!(1, entries.len());
    assert_eq!(Path::new("child"), entries[0].relative_path());
}

/// Verifies rooted directory enumeration starts when iteration advances.
#[test]
fn test_rooted_local_file_system_walker_defers_root_enumeration() {
    let directory = tempdir().expect("temporary root should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut walker = rooted
        .list(Path::new(""), &LocalListOptions::new())
        .expect("rooted walker should be created without enumerating");

    fs::write(directory.path().join("late-entry"), b"payload")
        .expect("entry should be created after walker construction");

    let entry = walker
        .next()
        .expect("late entry should be observed")
        .expect("late entry should be readable");
    assert_eq!(Path::new("late-entry"), entry.relative_path());
}

/// Verifies rooted writer publication and unified copy remain
/// descriptor-relative.
#[test]
fn test_rooted_local_file_system_writes_and_copies_file() {
    let directory = tempdir().expect("temporary root should be created");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");
    let mut writer = rooted
        .open_writer(
            Path::new("source"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("rooted writer should open");
    writer
        .write_all(b"payload")
        .expect("staging write should succeed");
    let _outcome = writer.commit().expect("rooted commit should succeed");

    let outcome = rooted
        .copy(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new(),
        )
        .expect("rooted copy should succeed");
    assert_eq!(1, outcome.stats().files());
    assert_eq!(
        b"payload",
        fs::read(directory.path().join("target"))
            .expect("copied target should exist")
            .as_slice(),
    );
}

/// Verifies that rooted overwrite publication replaces a final symlink entry.
#[cfg(unix)]
#[test]
fn test_rooted_local_file_system_writer_replaces_final_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let target = directory.path().join("target");
    fs::write(&referent, b"original").expect("referent should be written");
    symlink("referent", &target).expect("target symlink should be created");
    let root = RootedLocalFileSystem::open(directory.path())
        .expect("rooted filesystem should open");

    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = root
        .open_writer(Path::new("target"), &options)
        .expect("rooted writer should accept the final symlink");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    let outcome = writer.commit().expect("replacement should publish");

    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert!(
        fs::symlink_metadata(&target)
            .expect("target metadata should exist")
            .is_file(),
    );
    assert_eq!(
        b"original".to_vec(),
        fs::read(&referent).expect("referent should remain unchanged"),
    );
}

/// Verifies rooted create-new commit preserves a concurrently created entry.
#[test]
fn test_rooted_local_file_system_create_new_preserves_concurrent_target() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let root = RootedLocalFileSystem::open(directory.path())
        .expect("rooted filesystem should open");
    let mut writer = root
        .open_writer(
            Path::new("target"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("create-new staging should open for an absent target");
    writer
        .write_all(b"staged")
        .expect("staged bytes should be written");
    fs::write(&target, b"concurrent")
        .expect("concurrent target should be created");

    let error = writer
        .commit()
        .expect_err("rooted create-new must not replace a concurrent target");

    assert_eq!(
        qubit_local_files::LocalWriteFailureState::NotPublished,
        error.state(),
    );
    assert_eq!(
        b"concurrent",
        fs::read(&target)
            .expect("concurrent target should remain")
            .as_slice(),
    );
}

/// Verifies rooted deletion distinguishes files and recursive directories.
#[test]
fn test_rooted_local_file_system_deletes_entries() {
    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("tree"))
        .expect("tree should be created");
    fs::write(directory.path().join("tree/child"), b"x")
        .expect("child should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let outcome = rooted
        .delete_directory(
            Path::new("tree"),
            &LocalDeleteOptions::new().with_recursive(),
        )
        .expect("recursive rooted deletion should succeed");

    assert!(outcome.deleted());
    assert!(!directory.path().join("tree").exists());
}

/// Verifies the opened descriptor remains authoritative after diagnostic-path
/// rename.
#[test]
fn test_rooted_local_file_system_survives_root_path_rename() {
    let parent = tempdir().expect("temporary parent should be created");
    let original = parent.path().join("original");
    let renamed = parent.path().join("renamed");
    fs::create_dir(&original).expect("root fixture should be created");
    let rooted = RootedLocalFileSystem::open(&original)
        .expect("root authority should open");
    fs::rename(&original, &renamed).expect("diagnostic path should be renamed");

    let _outcome = rooted
        .create_directory(
            Path::new("nested"),
            &LocalCreateDirectoryOptions::new(),
        )
        .expect("descriptor-relative creation should still succeed");

    assert!(renamed.join("nested").is_dir());
    assert!(!original.exists());
}

/// Verifies rooted metadata and reader operations share descriptor-relative
/// authority.
#[test]
fn test_rooted_local_file_system_reads_regular_file() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("payload"), b"content")
        .expect("fixture should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let metadata = rooted
        .metadata(Path::new("payload"))
        .expect("metadata should be read");
    assert_eq!(LocalFileKind::File, metadata.kind());

    let mut reader = rooted
        .open_reader(Path::new("payload"), &LocalReadOptions::new())
        .expect("reader should open");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("reader should read fixture");
    assert_eq!("content", content);
}

/// Verifies rooted rename defaults to no-replace.
#[test]
fn test_rooted_local_file_system_rename_respects_overwrite() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"new")
        .expect("source should be written");
    fs::write(directory.path().join("target"), b"old")
        .expect("target should be written");
    let rooted = RootedLocalFileSystem::open(directory.path())
        .expect("root authority should open");

    let error = rooted
        .rename(
            Path::new("source"),
            Path::new("target"),
            &LocalRenameOptions::new(),
        )
        .expect_err("default rename must not replace");
    assert_eq!(LocalFileErrorKind::AlreadyExists, error.error().kind());

    let _outcome = rooted
        .rename(
            Path::new("source"),
            Path::new("target"),
            &LocalRenameOptions::new().with_overwrite(),
        )
        .expect("explicit overwrite should succeed");
    assert_eq!(
        b"new",
        fs::read(directory.path().join("target"))
            .expect("target should be replaced")
            .as_slice(),
    );
}
