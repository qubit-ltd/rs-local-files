// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalTempDirectory;
use qubit_local_files::LocalTempFile;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::policy::LocalPrivacyRequirement;

const CASE_ENV: &str = "QUBIT_TEMP_FD_FAILURE_CASE";

struct DescriptorBudget {
    original: libc::rlimit,
    files: Vec<fs::File>,
}

impl DescriptorBudget {
    fn new(available: usize) -> Self {
        let mut original = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `original` is writable storage for the resource limit.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut original) }, 0);
        let reduced = libc::rlimit {
            rlim_cur: original.rlim_cur.min(128),
            rlim_max: original.rlim_max,
        };
        // SAFETY: This runs only in an isolated child and preserves the hard limit.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &reduced) }, 0);
        let mut budget = Self {
            original,
            files: Vec::new(),
        };
        loop {
            match fs::File::open("/dev/null") {
                Ok(file) => budget.files.push(file),
                Err(error) => {
                    assert_eq!(error.raw_os_error(), Some(libc::EMFILE));
                    break;
                }
            }
        }
        assert!(budget.files.len() >= available);
        for _ in 0..available {
            drop(budget.files.pop().expect("reserved descriptor should exist"));
        }
        budget
    }
}

impl Drop for DescriptorBudget {
    fn drop(&mut self) {
        // SAFETY: Restores this child's original soft limit and unchanged hard limit.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &self.original) }, 0);
    }
}

#[derive(Debug)]
enum Temporary {
    File(LocalTempFile),
    Directory(LocalTempDirectory),
}

#[test]
fn test_temp_creation_recovers_from_each_descriptor_budget() {
    let executable = std::env::current_exe().expect("test executable should exist");
    for rooted in [false, true] {
        for directory in [false, true] {
            for private in [false, true] {
                for available in 0..=16 {
                    let case = format!("{rooted},{directory},{private},{available}");
                    let output = Command::new(&executable)
                        .args([
                            "--exact",
                            "test_temp_creation_descriptor_budget_child",
                            "--test-threads=1",
                        ])
                        .env(CASE_ENV, &case)
                        .output()
                        .expect("isolated descriptor budget test should start");
                    assert!(
                        output.status.success(),
                        "case {case}:\n{}\n{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
    }
}

#[test]
fn test_temp_creation_descriptor_budget_child() {
    let Ok(case) = std::env::var(CASE_ENV) else { return };
    let parts = case.split(',').collect::<Vec<_>>();
    let rooted = parts[0].parse::<bool>().expect("rooted flag");
    let directory = parts[1].parse::<bool>().expect("directory flag");
    let private = parts[2].parse::<bool>().expect("private flag");
    let available = parts[3].parse::<usize>().expect("descriptor budget");
    let fixture = tempfile::tempdir().expect("fixture should exist before exhausting descriptors");
    let native_parent = fs::canonicalize(fixture.path()).expect("fixture should resolve");
    fs::write(native_parent.join("sentinel"), b"protected user data").expect("sentinel should be written");
    let filesystem = if rooted {
        LocalFileSystem::rooted(&native_parent)
    } else {
        LocalFileSystem::host()
    }
    .expect("filesystem should open before exhausting descriptors");
    let parent = if rooted {
        Path::new("/")
    } else {
        native_parent.as_path()
    };
    let privacy = if private {
        LocalPrivacyRequirement::Required
    } else {
        LocalPrivacyRequirement::PlatformDefault
    };
    let budget = DescriptorBudget::new(available);
    let result = if directory {
        filesystem
            .create_temp_directory_with_options(
                &LocalTempDirectoryOptions::new()
                    .with_parent(parent)
                    .with_privacy(privacy)
                    .with_max_attempts(1),
            )
            .map(Temporary::Directory)
    } else {
        filesystem
            .create_temp_file_with_options(
                &LocalTempFileOptions::new()
                    .with_parent(parent)
                    .with_privacy(privacy)
                    .with_max_attempts(1),
            )
            .map(Temporary::File)
    };
    drop(budget);
    if available == 0 && (!directory || private || rooted) {
        assert!(result.is_err(), "creation must need at least one native descriptor");
    }
    if available == 16 {
        assert!(
            result.is_ok(),
            "sufficient descriptor capacity should permit creation: {result:?}"
        );
    }
    match result {
        Ok(Temporary::File(mut file)) => file.cleanup().expect("successful file should clean up after recovery"),
        Ok(Temporary::Directory(mut directory)) => directory
            .cleanup()
            .expect("successful directory should clean up after recovery"),
        Err(error) => {
            assert_eq!(
                error.operation(),
                if directory {
                    LocalFileOperation::CreateTempDirectory
                } else {
                    LocalFileOperation::CreateTempFile
                }
            );
            let mut source: Option<&(dyn Error + 'static)> = Some(&error);
            let mut found = false;
            while let Some(current) = source {
                if current
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.raw_os_error() == Some(libc::EMFILE))
                {
                    found = true;
                    break;
                }
                source = current.source();
            }
            assert!(found, "creation must retain native EMFILE: {error:?}");
            let (_, recovery) = error.into_temp_recovery_parts();
            if let Some(mut recovery) = recovery {
                if recovery.identity().is_some() {
                    recovery
                        .cleanup()
                        .expect("identified resource should clean up after descriptor recovery");
                } else {
                    assert_eq!(
                        recovery
                            .cleanup()
                            .expect_err("unidentified entries must not be removed by name")
                            .kind(),
                        LocalFileErrorKind::RequirementNotMet
                    );
                }
            }
        }
    }
    assert_eq!(
        fs::read(native_parent.join("sentinel")).expect("sentinel should survive failed creation"),
        b"protected user data"
    );
    // The same authority must remain usable after capacity has been restored.
    let mut retry = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
        .expect("creation should recover");
    retry.cleanup().expect("retried file should clean up");
    assert_eq!(
        fs::read(native_parent.join("sentinel")).expect("sentinel should survive retry cleanup"),
        b"protected user data"
    );
}

const FILESYSTEM_CASE_ENV: &str = "QUBIT_FILESYSTEM_FD_FAILURE_CASE";

#[test]
fn test_filesystem_operations_recover_from_descriptor_exhaustion() {
    let executable = std::env::current_exe().expect("test executable should exist");
    for rooted in [false, true] {
        for operation in [
            "observed",
            "snapshot",
            "prefix",
            "append",
            "canonicalize",
            "metadata",
            "target-metadata",
            "open-root",
        ] {
            for available in 0..=12 {
                let case = format!("{rooted},{operation},{available}");
                let output = Command::new(&executable)
                    .args(["--exact", "test_filesystem_descriptor_budget_child", "--test-threads=1"])
                    .env(FILESYSTEM_CASE_ENV, &case)
                    .output()
                    .expect("isolated filesystem operation should start");
                assert!(
                    output.status.success(),
                    "case {case}:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}

#[test]
fn test_filesystem_descriptor_budget_child() {
    use qubit_local_files::error::LocalFileError;
    use qubit_local_files::options::LocalObservedReadOptions;
    use qubit_local_files::options::LocalPathBindingOptions;
    use qubit_local_files::options::LocalSnapshotOptions;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;

    let Ok(case) = std::env::var(FILESYSTEM_CASE_ENV) else {
        return;
    };
    let parts = case.split(',').collect::<Vec<_>>();
    let rooted = parts[0].parse::<bool>().expect("rooted flag");
    let operation = parts[1];
    let available = parts[2].parse::<usize>().expect("descriptor budget");
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let native_root = fs::canonicalize(fixture.path()).expect("fixture should resolve");
    fs::create_dir_all(native_root.join("parent/child")).expect("nested fixture should exist");
    let native_path = native_root.join("parent/child/payload");
    fs::write(&native_path, b"protected user data").expect("payload should be written");
    let filesystem = if rooted {
        LocalFileSystem::rooted(&native_root)
    } else {
        LocalFileSystem::host()
    }
    .expect("filesystem should open before descriptor exhaustion");
    let path = if rooted {
        Path::new("/parent/child/payload")
    } else {
        native_path.as_path()
    };
    // Keep the structured error intact below so the test can inspect its native
    // source.
    #[allow(clippy::result_large_err)]
    let invoke = || -> Result<(), LocalFileError> {
        match operation {
            "observed" => filesystem
                .open_observed_reader(
                    path,
                    &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
                )
                .and_then(|mut reader| {
                    assert_eq!(
                        reader
                            .read_to_end_limited(32)
                            .expect("retained file needs no new descriptors to read"),
                        b"protected user data"
                    );
                    reader.verify_path_binding().map(|_| ())
                }),
            "snapshot" => filesystem
                .read_consistent_with_options(path, &LocalSnapshotOptions::builder(32).build())
                .map(|snapshot| assert_eq!(snapshot.bytes(), b"protected user data")),
            "prefix" => filesystem
                .read_prefix(path, 9)
                .map(|bytes| assert_eq!(bytes, b"protected")),
            "append" => filesystem
                .open_writer_with_options(path, &LocalWriteOptions::new(LocalWriteMode::Append))
                .map(drop),
            "canonicalize" => filesystem.canonicalize(path).map(|actual| assert_eq!(actual, path)),
            "metadata" => filesystem.metadata(path).map(|metadata| assert_eq!(metadata.len(), 19)),
            "target-metadata" => filesystem
                .target_metadata(path)
                .map(|metadata| assert_eq!(metadata.len(), 19)),
            "open-root" => LocalFileSystem::rooted(&native_root).map(drop),
            _ => panic!("unexpected operation: {operation}"),
        }
    };
    let budget = DescriptorBudget::new(available);
    let result = invoke();
    drop(budget);
    if available == 12 && operation != "observed" {
        assert!(
            result.is_ok(),
            "sufficient descriptors should permit {operation}: {result:?}"
        );
    }
    if let Err(error) = result {
        assert_eq!(error.kind(), LocalFileErrorKind::Io, "{error:?}");
        assert!(
            error.path().is_some(),
            "resource failure must retain the failing path: {error:?}"
        );
        let mut source: Option<&(dyn Error + 'static)> = Some(&error);
        let mut native_emfile = false;
        while let Some(current) = source {
            if current
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.raw_os_error() == Some(libc::EMFILE))
            {
                native_emfile = true;
                break;
            }
            source = current.source();
        }
        assert!(native_emfile, "operation must retain native EMFILE: {error:?}");
    }
    assert_eq!(
        fs::read(&native_path).expect("payload should survive resource errors"),
        b"protected user data"
    );
    invoke().expect("same filesystem should recover after descriptor capacity returns");
    assert_eq!(
        fs::read(&native_path).expect("retry should preserve payload"),
        b"protected user data"
    );
}
