// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalTreeEntry;
use qubit_local_files::directory::LocalTreeValidationOptions;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreePublishOptions;
use qubit_local_files::outcome::LocalFileKind;
use qubit_local_files::outcome::LocalTreeMutationState;
use qubit_local_files::policy::LocalOwnerAccessPolicy;

fn make_stage(
    rooted: bool,
) -> (
    tempfile::TempDir,
    LocalFileSystem,
    qubit_local_files::directory::LocalStagedDirectory,
    PathBuf,
) {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = if rooted {
        LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem")
    } else {
        LocalFileSystem::host().expect("Host filesystem")
    };
    let parent_path = if rooted { Path::new("/") } else { fixture.path() };
    let parent = filesystem
        .open_directory_with_options(parent_path, &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let diagnostic_root = fixture.path().join(stage.relative_name());
    (fixture, filesystem, stage, diagnostic_root)
}

fn entry(path: &str, kind: LocalFileKind) -> LocalTreeEntry {
    LocalTreeEntry::new(PathBuf::from(path), kind).expect("valid expected entry")
}

fn write_file(stage: &mut qubit_local_files::directory::LocalStagedDirectory, path: &str) {
    stage
        .write_file_new(Path::new(path), &mut &b"x"[..], 1)
        .expect("file write");
}

fn assert_requirement(error: &qubit_local_files::LocalFileError, expected_path: &Path, reason: &str) {
    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert_eq!(error.path(), Some(expected_path));
    assert!(error.reason().expect("reason").contains(reason));
}

#[test]
fn diagnostics_cover_host_and_rooted_exact_set_failures() {
    for rooted in [false, true] {
        let (_fixture, _filesystem, mut stage, root) = make_stage(rooted);
        write_file(&mut stage, "extra");
        let error = stage
            .verify_entries(&[], &LocalTreeValidationOptions::new())
            .expect_err("unexpected entry");
        assert_requirement(&error, &root.join("extra"), "unexpected");
        stage.cleanup().expect("cleanup");

        let (_fixture, _filesystem, stage, root) = make_stage(rooted);
        let error = stage
            .verify_entries(
                &[entry("missing", LocalFileKind::File)],
                &LocalTreeValidationOptions::new(),
            )
            .expect_err("missing expected entry");
        assert_requirement(&error, &root.join("missing"), "missing");
        stage.cleanup().expect("cleanup");

        let (_fixture, _filesystem, mut stage, root) = make_stage(rooted);
        write_file(&mut stage, "wrong-kind");
        let error = stage
            .verify_entries(
                &[entry("wrong-kind", LocalFileKind::Directory)],
                &LocalTreeValidationOptions::new(),
            )
            .expect_err("entry kind mismatch");
        assert_requirement(&error, &root.join("wrong-kind"), "kind mismatch");
        assert!(error.to_string().contains("expected Directory, observed File"));
        stage.cleanup().expect("cleanup");

        let (_fixture, _filesystem, stage, root) = make_stage(rooted);
        let duplicate = [entry("same", LocalFileKind::File), entry("same", LocalFileKind::File)];
        let error = stage
            .verify_entries(&duplicate, &LocalTreeValidationOptions::new())
            .expect_err("duplicate expected path");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(error.path(), Some(root.join("same").as_path()));
        assert!(error.reason().expect("reason").contains("duplicate"));
        stage.cleanup().expect("cleanup");
    }
}

#[test]
fn diagnostics_include_depth_and_name_budget_facts() {
    for rooted in [false, true] {
        let (_fixture, _filesystem, mut stage, root) = make_stage(rooted);
        std::fs::create_dir(root.join("deep")).expect("nested directory");
        write_file(&mut stage, "deep/file");
        let error = stage
            .verify_entries(
                &[
                    entry("deep", LocalFileKind::Directory),
                    entry("deep/file", LocalFileKind::File),
                ],
                &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(1)),
            )
            .expect_err("depth exceeded");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.path(), Some(root.join("deep/file").as_path()));
        let facts = error.resource_limit_error().expect("depth facts");
        assert_eq!(facts.resource(), LocalResourceKind::Depth);
        assert_eq!(facts.limit(), 1);
        assert_eq!(facts.requested(), 2);
        assert_eq!(facts.observed(), Some(2));
        assert!(error.reason().expect("reason").contains("depth"));
        stage.cleanup().expect("cleanup");

        let (_fixture, _filesystem, stage, root) = make_stage(rooted);
        let expected = [entry("long", LocalFileKind::File)];
        let error = stage
            .verify_entries(
                &expected,
                &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_seen_name_bytes(3)),
            )
            .expect_err("expected name-byte preflight exceeded");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.path(), Some(root.join("long").as_path()));
        let facts = error.resource_limit_error().expect("name-byte facts");
        assert_eq!(facts.resource(), LocalResourceKind::SeenNameBytes);
        assert_eq!(facts.limit(), 3);
        assert_eq!(facts.requested(), 4);
        assert_eq!(facts.observed(), Some(4));
        assert!(error.reason().expect("reason").contains("name"));
        stage.cleanup().expect("cleanup");
    }
}

#[test]
fn validation_deadline_overflow_is_reported_as_resource_limit() {
    let (_fixture, _filesystem, stage, root) = make_stage(false);
    let error = stage
        .verify_entries(
            &[],
            &LocalTreeValidationOptions::new()
                .with_limits(LocalListOptions::new().with_deadline(std::time::Duration::MAX)),
        )
        .expect_err("unrepresentable validation deadline");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.path(), Some(root.as_path()));
    assert!(error.reason().expect("reason").contains("deadline overflowed"));
    stage.cleanup().expect("cleanup");
}

#[test]
fn validation_reports_entry_budget_and_walker_start_failures() {
    let (_fixture, _filesystem, mut stage, _root) = make_stage(false);
    write_file(&mut stage, "entry");

    let entry_error = stage
        .verify_entries(
            &[],
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_entries(0)),
        )
        .expect_err("expected entry count exceeds its budget");
    assert_eq!(entry_error.kind(), LocalFileErrorKind::ResourceLimit);
    assert!(entry_error.path().is_some());
    assert_eq!(
        entry_error.resource_limit_error().expect("entry facts").resource(),
        LocalResourceKind::Entry
    );

    let walker_error = stage
        .verify_entries(
            &[],
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_open_directories(0)),
        )
        .expect_err("walker cannot start with a zero directory budget");
    assert_eq!(walker_error.kind(), LocalFileErrorKind::InvalidOptions);
    assert!(
        walker_error
            .reason()
            .expect("reason")
            .contains("tree traversal could not start")
    );
    stage.cleanup().expect("cleanup");
}

#[test]
fn publish_validation_diagnostic_preserves_state_and_recovery() {
    let (_fixture, filesystem, mut stage, root) = make_stage(false);
    write_file(&mut stage, "extra");
    let parent = filesystem
        .open_directory_with_options(_fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let error = stage
        .publish_new(
            &parent,
            std::ffi::OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_validation(LocalTreeValidationOptions::new().with_exact_entries(vec![])),
        )
        .expect_err("validation rejects extra entry");
    assert_requirement(error.cause(), &root.join("extra"), "unexpected");
    assert_eq!(error.mutation_state(), LocalTreeMutationState::NotStarted);
    error
        .into_stage()
        .expect("validation failure retains recoverable stage")
        .cleanup()
        .expect("recovered stage cleanup");
}

#[cfg(unix)]
#[test]
fn rejected_symlink_has_entry_diagnostic() {
    use std::os::unix::fs::symlink;

    let (_fixture, _filesystem, stage, root) = make_stage(false);
    symlink("target", root.join("link")).expect("symlink");
    let error = stage
        .verify_entries(
            &[entry("link", LocalFileKind::Symlink)],
            &LocalTreeValidationOptions::new().with_reject_symlinks(),
        )
        .expect_err("symlink rejected");
    assert_requirement(&error, &root.join("link"), "symbolic");
    stage.cleanup().expect("cleanup");
}

#[cfg(unix)]
#[test]
fn hard_link_and_special_entry_failures_have_context() {
    let (fixture, _filesystem, mut stage, root) = make_stage(false);
    write_file(&mut stage, "linked");
    std::fs::hard_link(root.join("linked"), fixture.path().join("outside-link")).expect("external hard link");
    let error = stage
        .verify_entries(
            &[entry("linked", LocalFileKind::File)],
            &LocalTreeValidationOptions::new().with_reject_external_hardlinks(),
        )
        .expect_err("external hard link rejected");
    assert_requirement(&error, &root, "external hard links");
    stage.cleanup().expect("cleanup");

    let (_fixture, _filesystem, stage, root) = make_stage(false);
    let fifo = std::ffi::CString::new(root.join("fifo").as_os_str().as_encoded_bytes()).expect("FIFO path has no NUL");
    // SAFETY: `fifo` is a valid NUL-terminated path to a new fixture entry.
    let result = unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) };
    assert_eq!(result, 0, "FIFO fixture creation");
    let error = stage
        .verify_entries(&[], &LocalTreeValidationOptions::new().with_reject_special_files())
        .expect_err("special file rejected");
    assert_requirement(&error, &root.join("fifo"), "special file");
    stage.cleanup().expect("cleanup");
}

#[cfg(unix)]
#[test]
fn owner_access_policy_failures_include_root_and_entry_paths() {
    use std::os::unix::fs::PermissionsExt;

    let (_fixture, _filesystem, stage, root) = make_stage(false);
    let root_mode = std::fs::metadata(&root).expect("root metadata").permissions().mode() & 0o777;
    let root_error = stage
        .verify_entries(
            &[],
            &LocalTreeValidationOptions::new()
                .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(root_mode ^ 0o100, 0o644)),
        )
        .expect_err("unexpected root access mode");
    assert_eq!(root_error.path(), Some(root.as_path()));
    assert!(root_error.reason().expect("root reason").contains("owner-access"));
    stage.cleanup().expect("cleanup root stage");

    let (_fixture, _filesystem, mut stage, root) = make_stage(false);
    write_file(&mut stage, "private-file");
    let root_mode = std::fs::metadata(&root).expect("root metadata").permissions().mode() & 0o777;
    let file_path = root.join("private-file");
    let file_mode = std::fs::metadata(&file_path)
        .expect("file metadata")
        .permissions()
        .mode()
        & 0o777;
    let entry_error = stage
        .verify_entries(
            &[entry("private-file", LocalFileKind::File)],
            &LocalTreeValidationOptions::new()
                .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(root_mode, file_mode ^ 0o100)),
        )
        .expect_err("unexpected file access mode");
    assert_eq!(entry_error.path(), Some(root.join("private-file").as_path()));
    assert!(entry_error.reason().expect("entry reason").contains("owner-access"));
    stage.cleanup().expect("cleanup entry stage");
}
