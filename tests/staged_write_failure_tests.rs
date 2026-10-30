// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::VecDeque;
use std::io;
use std::io::Read;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::directory::LocalStagedDirectory;
use qubit_local_files::error::LocalFileErrorKind;
#[cfg(all(feature = "test-support", unix))]
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
#[cfg(target_os = "linux")]
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalPrivacyRequirement;
use tempfile::TempDir;

fn parent(path: &Path) -> LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(path, &LocalDirectoryOpenOptions::new())
        .expect("parent authority")
}

fn stage(parent: &LocalDirectoryHandle, options: &LocalTempDirectoryOptions) -> LocalStagedDirectory {
    parent.create_staged_directory(options).expect("staged directory")
}

fn native_stage(scratch: &TempDir, stage: &LocalStagedDirectory) -> std::path::PathBuf {
    scratch.path().join(stage.relative_name())
}

struct FailingInput;

impl Read for FailingInput {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("source failure"))
    }
}

#[cfg(all(feature = "test-support", unix))]
#[test]
fn test_source_failure_retains_staging_cleanup_failure() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
    let stage_path = native_stage(&scratch, &stage);
    let fault = install_test_fault("atomic-install-unlink-persistent").expect("fault guard");

    let error = stage
        .write_file_new(Path::new("payload"), &mut FailingInput, 64)
        .expect_err("source fails");

    assert_eq!(error.operation(), LocalFileOperation::Read);
    assert!(error.io_error().expect("source").to_string().contains("source failure"));
    assert!(error.cleanup_error().is_some(), "cleanup failure remains observable");
    assert_eq!(1, std::fs::read_dir(&stage_path).expect("residual listing").count());
    drop(fault);
    stage.cleanup().expect("the caller retains the stage cleanup owner");
    assert!(!stage_path.exists());
}

#[cfg(all(feature = "test-support", unix))]
#[test]
fn test_resource_limit_retains_staging_cleanup_failure() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
    let stage_path = native_stage(&scratch, &stage);
    let fault = install_test_fault("atomic-install-unlink-persistent").expect("fault guard");

    let error = stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 0)
        .expect_err("zero maximum rejects nonempty input");

    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert!(error.cleanup_error().is_some());
    assert_eq!(1, std::fs::read_dir(&stage_path).expect("residual listing").count());
    drop(fault);
    stage
        .cleanup()
        .expect("stage cleanup should recover after the fault is removed");
    assert!(!stage_path.exists());
}

#[test]
fn test_successful_abort_error_path_leaves_no_staging_residue() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
    let stage_path = native_stage(&scratch, &stage);

    let error = stage
        .write_file_new(Path::new("payload"), &mut FailingInput, 64)
        .expect_err("source fails");

    assert_eq!(error.kind(), LocalFileErrorKind::Io);
    assert!(error.cleanup_error().is_none());
    assert_eq!(0, std::fs::read_dir(&stage_path).expect("empty stage").count());
    stage.cleanup().expect("stage should clean up");
}

enum InputEvent {
    Interrupted,
    Data(&'static [u8]),
    Error(io::ErrorKind),
}

struct ScriptedInput {
    events: VecDeque<InputEvent>,
    remaining: &'static [u8],
}

impl Read for ScriptedInput {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        loop {
            if !self.remaining.is_empty() {
                let count = output.len().min(self.remaining.len());
                output[..count].copy_from_slice(&self.remaining[..count]);
                self.remaining = &self.remaining[count..];
                return Ok(count);
            }
            match self.events.pop_front() {
                Some(InputEvent::Interrupted) => return Err(io::ErrorKind::Interrupted.into()),
                Some(InputEvent::Data(data)) => self.remaining = data,
                Some(InputEvent::Error(kind)) => return Err(kind.into()),
                None => return Ok(0),
            }
        }
    }
}

#[test]
fn test_staged_write_retries_input_interrupted_for_default_and_private_stages() {
    let options = [
        LocalTempDirectoryOptions::new(),
        LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        #[cfg(target_os = "linux")]
        LocalTempDirectoryOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600)),
    ];
    for options in options {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let parent = parent(scratch.path());
        let mut stage = stage(&parent, &options);
        let mut input = ScriptedInput {
            events: VecDeque::from([
                InputEvent::Interrupted,
                InputEvent::Data(b"pay"),
                InputEvent::Interrupted,
                InputEvent::Interrupted,
                InputEvent::Data(b"load"),
            ]),
            remaining: b"",
        };

        stage
            .write_file_new(Path::new("payload"), &mut input, 7)
            .expect("initial, partial-progress, and repeated interruption should be retried");

        assert_eq!(
            b"payload",
            std::fs::read(native_stage(&scratch, &stage).join("payload"))
                .expect("payload")
                .as_slice()
        );
        stage.cleanup().expect("stage cleanup");
    }
}

#[test]
fn test_staged_write_does_not_retry_would_block() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
    let stage_path = native_stage(&scratch, &stage);
    let mut input = ScriptedInput {
        events: VecDeque::from([InputEvent::Error(io::ErrorKind::WouldBlock)]),
        remaining: b"",
    };

    let error = stage
        .write_file_new(Path::new("payload"), &mut input, 16)
        .expect_err("WouldBlock is not retried");

    assert_eq!(io::ErrorKind::WouldBlock, error.io_error_kind());
    assert!(!stage_path.join("payload").exists());
    stage.cleanup().expect("stage cleanup");
}

#[cfg(all(feature = "test-support", unix))]
#[test]
fn test_staged_write_retries_output_interrupted() {
    use qubit_local_files::test_support::install_test_fault;

    let scratch = tempfile::tempdir().expect("scratch directory");
    let parent = parent(scratch.path());
    let mut stage = stage(&parent, &LocalTempDirectoryOptions::new());
    let fault = install_test_fault("local-writer-interrupted").expect("fault");

    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 7)
        .expect("output interruption should be retried");

    drop(fault);
    assert_eq!(
        b"payload",
        std::fs::read(native_stage(&scratch, &stage).join("payload"))
            .expect("payload")
            .as_slice()
    );
    stage.cleanup().expect("stage cleanup");
}
