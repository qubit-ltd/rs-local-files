// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use qubit_local_files::LocalCopyFailure;
use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalCopyTypeConflictPolicy;
use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalResourceKind;
use tempfile::TempDir;
use tempfile::tempdir;

#[derive(Clone, Copy, Debug)]
enum Backend {
    Host,
    Rooted,
}

impl Backend {
    fn all() -> [Self; 2] {
        [Self::Host, Self::Rooted]
    }
}

struct CopyFixture {
    _directory: TempDir,
    filesystem: LocalFileSystem,
    source: PathBuf,
    target: PathBuf,
}

fn copy_fixture(backend: Backend) -> CopyFixture {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    fs::create_dir(&source).expect("source directory should be created");
    fs::write(source.join("child"), b"payload").expect("source payload should be written");
    match backend {
        Backend::Host => CopyFixture {
            source,
            target: directory.path().join("target"),
            filesystem: LocalFileSystem::host().expect("Host filesystem should open"),
            _directory: directory,
        },
        Backend::Rooted => CopyFixture {
            source: PathBuf::from("source"),
            target: PathBuf::from("target"),
            filesystem: LocalFileSystem::rooted(directory.path()).expect("root authority should open"),
            _directory: directory,
        },
    }
}

fn assert_resource_limit(backend: Backend, options: LocalCopyOptions, resource: LocalResourceKind) -> LocalCopyFailure {
    let fixture = copy_fixture(backend);
    let failure = fixture
        .filesystem
        .copy_with_options(&fixture.source, &fixture.target, &options.with_tree_source())
        .expect_err("copy budget should reject the fixture");
    assert_eq!(
        LocalFileErrorKind::ResourceLimit,
        failure.error().kind(),
        "{backend:?} should report a resource limit",
    );
    assert_eq!(
        resource,
        failure
            .error()
            .resource_limit_error()
            .expect("copy budget facts should be retained")
            .resource(),
        "{backend:?} should retain the exhausted resource",
    );
    failure
}

#[test]
fn test_copy_budget_matrix_enforces_max_depth() {
    for backend in Backend::all() {
        let _ = assert_resource_limit(
            backend,
            LocalCopyOptions::new().with_max_depth(0),
            LocalResourceKind::Depth,
        );
    }
}

#[test]
fn test_copy_budget_matrix_enforces_max_entries() {
    for backend in Backend::all() {
        let _ = assert_resource_limit(
            backend,
            LocalCopyOptions::new().with_max_entries(0),
            LocalResourceKind::Entry,
        );
    }
}

#[test]
fn test_copy_entry_budget_counts_a_single_source_file() {
    for backend in Backend::all() {
        let directory = tempdir().expect("temporary directory should be created");
        fs::write(directory.path().join("source"), b"payload").expect("source file should be written");
        let (filesystem, source, target) = match backend {
            Backend::Host => (
                LocalFileSystem::host().expect("Host filesystem should open"),
                directory.path().join("source"),
                directory.path().join("target"),
            ),
            Backend::Rooted => (
                LocalFileSystem::rooted(directory.path()).expect("root authority should open"),
                PathBuf::from("source"),
                PathBuf::from("target"),
            ),
        };

        let failure = filesystem
            .copy_with_options(&source, &target, &LocalCopyOptions::new().with_max_entries(0))
            .expect_err("a zero-entry budget must reject one source file");

        assert_eq!(LocalFileErrorKind::ResourceLimit, failure.error().kind());
        assert_eq!(
            Some(LocalResourceKind::Entry),
            failure.error().resource_limit_error().map(|error| error.resource()),
        );
        assert!(!directory.path().join("target").exists());
    }
}

#[test]
fn test_copy_budget_matrix_enforces_max_bytes() {
    for backend in Backend::all() {
        let failure = assert_resource_limit(
            backend,
            LocalCopyOptions::new().with_max_bytes(0),
            LocalResourceKind::CopiedBytes,
        );
        let facts = failure
            .error()
            .resource_limit_error()
            .expect("byte budget facts should be retained");
        assert_eq!(0, facts.limit());
        assert_eq!(0, facts.remaining());
        assert!(facts.requested() > 0);
    }
}

#[test]
fn test_copy_budget_matrix_enforces_max_open_directories() {
    for backend in Backend::all() {
        let _ = assert_resource_limit(
            backend,
            LocalCopyOptions::new().with_max_open_directories(0),
            LocalResourceKind::OpenDirectory,
        );
    }
}

#[test]
fn test_copy_budget_matrix_enforces_deadline() {
    for backend in Backend::all() {
        let fixture = copy_fixture(backend);
        let failure = fixture
            .filesystem
            .copy_with_options(
                &fixture.source,
                &fixture.target,
                &LocalCopyOptions::new().with_tree_source().with_deadline(Duration::ZERO),
            )
            .expect_err("an immediate copy deadline should expire");
        assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
        assert_eq!(
            Some(std::io::ErrorKind::TimedOut),
            failure.error().io_error().map(std::io::Error::kind),
        );
    }
}

#[test]
fn test_copy_deadline_precedes_type_conflict_skip_outcomes() {
    for backend in Backend::all() {
        let directory = tempdir().expect("temporary directory should be created");
        fs::write(directory.path().join("source"), b"payload").expect("source file should be written");
        fs::create_dir(directory.path().join("target")).expect("target directory should be created");
        let (filesystem, source, target) = match backend {
            Backend::Host => (
                LocalFileSystem::host().expect("Host filesystem should open"),
                directory.path().join("source"),
                directory.path().join("target"),
            ),
            Backend::Rooted => (
                LocalFileSystem::rooted(directory.path()).expect("root authority should open"),
                PathBuf::from("source"),
                PathBuf::from("target"),
            ),
        };

        let failure = filesystem
            .copy_with_options(
                &source,
                &target,
                &LocalCopyOptions::new()
                    .with_type_conflict(LocalCopyTypeConflictPolicy::Skip)
                    .with_deadline(Duration::ZERO),
            )
            .expect_err("an expired deadline must precede a skip outcome");

        assert_eq!(
            Some(std::io::ErrorKind::TimedOut),
            failure.error().io_error().map(std::io::Error::kind),
        );
    }
}

#[cfg(unix)]
#[test]
fn test_copy_deadline_applies_to_final_symlink_entries() {
    use std::os::unix::fs::symlink;

    for backend in Backend::all() {
        let directory = tempdir().expect("temporary directory should be created");
        fs::write(directory.path().join("referent"), b"payload").expect("referent should be written");
        symlink("referent", directory.path().join("source")).expect("source symbolic link should be created");
        let (filesystem, source, target) = match backend {
            Backend::Host => (
                LocalFileSystem::host().expect("Host filesystem should open"),
                directory.path().join("source"),
                directory.path().join("target"),
            ),
            Backend::Rooted => (
                LocalFileSystem::rooted(directory.path()).expect("root authority should open"),
                PathBuf::from("source"),
                PathBuf::from("target"),
            ),
        };

        let failure = filesystem
            .copy_with_options(&source, &target, &LocalCopyOptions::new().with_deadline(Duration::ZERO))
            .expect_err("an immediate deadline should reject a final symlink copy");

        assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
        assert_eq!(
            Some(std::io::ErrorKind::TimedOut),
            failure.error().io_error().map(std::io::Error::kind),
        );
        assert!(
            !directory.path().join("target").exists(),
            "an expired copy must not publish its target",
        );
    }
}

#[test]
fn test_copy_budget_matrix_rejects_unrepresentable_deadline() {
    for backend in Backend::all() {
        let fixture = copy_fixture(backend);
        let failure = fixture
            .filesystem
            .copy_with_options(
                &fixture.source,
                &fixture.target,
                &LocalCopyOptions::new().with_tree_source().with_deadline(Duration::MAX),
            )
            .expect_err("an unrepresentable deadline should be invalid");
        assert_eq!(LocalFileErrorKind::InvalidOptions, failure.error().kind());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn test_copy_backends_enforce_max_bytes_against_actual_stream_length() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let source = Path::new("/proc/self/cmdline");
    assert_eq!(
        0,
        fs::metadata(source)
            .expect("procfs source metadata should be readable")
            .len(),
    );

    let failure = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(source, &target, &LocalCopyOptions::new().with_max_bytes(0))
        .expect_err("actual source bytes must exceed the zero budget");

    assert_eq!(LocalFileErrorKind::ResourceLimit, failure.error().kind());
    assert!(!target.exists(), "failed staging must not publish a target");

    let rooted_source = PathBuf::from(format!("proc/{}/cmdline", std::process::id()));
    let rooted_target = target
        .strip_prefix(Path::new("/"))
        .expect("temporary target should be absolute");
    let failure = LocalFileSystem::rooted(Path::new("/"))
        .expect("filesystem root authority should open")
        .copy_with_options(
            &rooted_source,
            rooted_target,
            &LocalCopyOptions::new().with_max_bytes(0),
        )
        .expect_err("Rooted actual source bytes must exceed the zero budget");

    assert_eq!(LocalFileErrorKind::ResourceLimit, failure.error().kind());
    assert!(!target.exists(), "failed staging must not publish a target");
}

#[cfg(unix)]
#[test]
fn test_rooted_copy_preserves_final_and_nested_symlink_entries() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("source")).expect("source directory should be created");
    fs::write(directory.path().join("source/referent"), b"payload").expect("referent should be written");
    symlink("referent", directory.path().join("source/link")).expect("nested source link should be created");
    symlink("source/referent", directory.path().join("final-link")).expect("final source link should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

    let _ = rooted
        .copy_with_options(
            Path::new("final-link"),
            Path::new("final-copy"),
            &LocalCopyOptions::new(),
        )
        .expect("final symlink should be copied as a link entry");
    let _ = rooted
        .copy_with_options(
            Path::new("source"),
            Path::new("tree-copy"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("nested symlink should be copied as a link entry");

    assert_eq!(
        PathBuf::from("source/referent"),
        fs::read_link(directory.path().join("final-copy")).expect("final copied link should be readable"),
    );
    assert_eq!(
        PathBuf::from("referent"),
        fs::read_link(directory.path().join("tree-copy/link")).expect("nested copied link should be readable"),
    );
}
