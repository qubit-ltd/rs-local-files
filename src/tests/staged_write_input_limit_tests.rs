// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::Path;

use crate::LocalFileSystem;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::policy::LocalOwnerAccessPolicy;
use crate::policy::LocalPrivacyRequirement;

struct RecordingInput {
    bytes: Vec<u8>,
    position: usize,
    requests: Vec<usize>,
    returns: Vec<usize>,
    short_read: Option<usize>,
    error_at_position: Option<(usize, io::ErrorKind)>,
}

impl RecordingInput {
    fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            position: 0,
            requests: Vec::new(),
            returns: Vec::new(),
            short_read: None,
            error_at_position: None,
        }
    }

    fn with_short_read(mut self, maximum: usize) -> Self {
        self.short_read = Some(maximum);
        self
    }

    fn with_error_at(mut self, position: usize, kind: io::ErrorKind) -> Self {
        self.error_at_position = Some((position, kind));
        self
    }
}

impl qubit_io::Input for RecordingInput {
    type Item = u8;

    unsafe fn read_unchecked(
        &mut self,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> io::Result<usize> {
        self.requests.push(count);
        if self
            .error_at_position
            .is_some_and(|(position, _)| position == self.position)
        {
            let (_, kind) = self.error_at_position.take().expect("error was configured");
            return Err(io::Error::new(kind, "configured input error"));
        }
        let available = self.bytes.len() - self.position;
        let count = count
            .min(available)
            .min(self.short_read.unwrap_or(usize::MAX));
        output[index..index + count]
            .copy_from_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        self.returns.push(count);
        Ok(count)
    }
}

fn stage_with_options(
    options: LocalTempDirectoryOptions,
) -> (tempfile::TempDir, crate::directory::LocalStagedDirectory) {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let parent = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let stage = parent
        .create_staged_directory(&options)
        .expect("stage should be created");
    (fixture, stage)
}

fn ordinary_stage() -> (tempfile::TempDir, crate::directory::LocalStagedDirectory) {
    stage_with_options(LocalTempDirectoryOptions::new())
}

#[test]
fn over_limit_reads_only_the_first_unaccepted_byte() {
    let (fixture, mut stage) = ordinary_stage();
    let stage_path = fixture.path().join(stage.relative_name());
    stage
        .write_file_new(
            Path::new("kept"),
            &mut RecordingInput::new(b"seed".to_vec()),
            4,
        )
        .expect("existing stage file should be created");
    let mut input = RecordingInput::new(b"longer".to_vec());

    let error = stage
        .write_file_new(Path::new("payload"), &mut input, 1)
        .expect_err("input longer than the limit must fail");

    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(input.requests, vec![1, 1]);
    assert_eq!(input.returns, vec![1, 1]);
    assert_eq!(input.position, 2);
    assert!(stage_path.join("kept").exists());
    assert_eq!(
        std::fs::read(stage_path.join("kept")).expect("seed remains"),
        b"seed"
    );
    assert!(!stage_path.join("payload").exists());
    stage.cleanup().expect("stage cleanup should succeed");
    assert!(!stage_path.exists());
}

#[test]
fn quota_matrix_preserves_exact_eof_probe_behavior() {
    let (fixture, mut stage) = ordinary_stage();
    let root = fixture.path().join(stage.relative_name());

    let mut empty = RecordingInput::new(Vec::new());
    stage
        .write_file_new(Path::new("empty"), &mut empty, 0)
        .expect("empty input fits");
    assert_eq!(empty.requests, vec![1]);
    assert_eq!(empty.position, 0);
    assert_eq!(
        std::fs::read(root.join("empty")).expect("empty file exists"),
        b""
    );

    let mut over_zero = RecordingInput::new(b"x".to_vec());
    let error = stage
        .write_file_new(Path::new("over-zero"), &mut over_zero, 0)
        .expect_err("nonempty input exceeds zero");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(over_zero.requests, vec![1]);
    assert_eq!(over_zero.returns, vec![1]);
    assert_eq!(over_zero.position, 1);
    assert!(!root.join("over-zero").exists());

    let mut exact = RecordingInput::new(b"x".to_vec());
    stage
        .write_file_new(Path::new("exact"), &mut exact, 1)
        .expect("exact limit fits");
    assert_eq!(exact.requests, vec![1, 1]);
    assert_eq!(exact.position, 1);
    assert_eq!(
        std::fs::read(root.join("exact")).expect("file exists"),
        b"x"
    );

    let mut block_plus_one = RecordingInput::new(vec![b'a'; 8193]);
    let error = stage
        .write_file_new(Path::new("block-plus-one"), &mut block_plus_one, 8192)
        .expect_err("extra byte exceeds limit");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(block_plus_one.requests, vec![8192, 1]);
    assert_eq!(block_plus_one.returns, vec![8192, 1]);
    assert_eq!(block_plus_one.position, 8193);
    assert!(!root.join("block-plus-one").exists());

    let mut exact_block_plus_one = RecordingInput::new(vec![b'b'; 8193]);
    stage
        .write_file_new(
            Path::new("exact-block-plus-one"),
            &mut exact_block_plus_one,
            8193,
        )
        .expect("exact content fits");
    assert_eq!(exact_block_plus_one.requests, vec![8192, 1, 1]);
    assert_eq!(exact_block_plus_one.returns, vec![8192, 1, 0]);
    assert_eq!(exact_block_plus_one.position, 8193);
    assert_eq!(
        std::fs::read(root.join("exact-block-plus-one"))
            .expect("file exists")
            .len(),
        8193
    );
    stage.cleanup().expect("stage cleanup should succeed");
}

#[test]
fn short_reads_and_interrupted_data_reads_retain_request_budget() {
    let (fixture, mut stage) = ordinary_stage();
    let mut short = RecordingInput::new(b"abcdef".to_vec()).with_short_read(2);
    stage
        .write_file_new(Path::new("short"), &mut short, 6)
        .expect("short reads eventually finish");
    assert_eq!(short.requests, vec![6, 4, 2, 1]);
    assert_eq!(short.returns, vec![2, 2, 2, 0]);
    assert_eq!(short.position, 6);
    assert_eq!(
        std::fs::read(fixture.path().join(stage.relative_name()).join("short"))
            .expect("file exists"),
        b"abcdef"
    );

    let mut interrupted =
        RecordingInput::new(b"abcd".to_vec()).with_error_at(0, io::ErrorKind::Interrupted);
    stage
        .write_file_new(Path::new("interrupted"), &mut interrupted, 4)
        .expect("interrupted read retries");
    assert_eq!(interrupted.requests, vec![4, 4, 1]);
    assert_eq!(interrupted.returns, vec![4, 0]);
    assert_eq!(interrupted.position, 4);
    stage.cleanup().expect("stage cleanup should succeed");
}

#[test]
fn probe_interrupted_and_non_interrupted_errors_preserve_stage() {
    let (fixture, mut stage) = ordinary_stage();
    let root = fixture.path().join(stage.relative_name());
    stage
        .write_file_new(
            Path::new("kept"),
            &mut RecordingInput::new(b"ok".to_vec()),
            2,
        )
        .expect("seed file writes");

    let mut interrupted_probe =
        RecordingInput::new(b"x".to_vec()).with_error_at(1, io::ErrorKind::Interrupted);
    stage
        .write_file_new(Path::new("probe-interrupted"), &mut interrupted_probe, 1)
        .expect("probe retries to EOF");
    assert_eq!(interrupted_probe.requests, vec![1, 1, 1]);
    assert_eq!(interrupted_probe.returns, vec![1, 0]);
    assert_eq!(interrupted_probe.position, 1);

    let mut failed_probe =
        RecordingInput::new(b"x".to_vec()).with_error_at(1, io::ErrorKind::BrokenPipe);
    let error = stage
        .write_file_new(Path::new("probe-error"), &mut failed_probe, 1)
        .expect_err("probe error is returned");
    assert_eq!(error.operation(), LocalFileOperation::Read);
    assert_eq!(error.io_error_kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(failed_probe.requests, vec![1, 1]);
    assert_eq!(failed_probe.position, 1);
    assert!(root.join("kept").exists());
    assert!(!root.join("probe-error").exists());
    stage.cleanup().expect("retained stage can be cleaned");
    assert!(!root.exists());
}

#[test]
fn required_privacy_stage_uses_limited_input_reads() {
    let options = LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required);
    let (fixture, mut stage) = stage_with_options(options);
    let root = fixture.path().join(stage.relative_name());

    let mut empty = RecordingInput::new(Vec::new());
    stage
        .write_file_new(Path::new("empty"), &mut empty, 0)
        .expect("empty input fits");
    assert_eq!(empty.requests, vec![1]);
    assert_eq!(empty.position, 0);

    let mut exact = RecordingInput::new(b"x".to_vec());
    stage
        .write_file_new(Path::new("exact"), &mut exact, 1)
        .expect("exact content fits");
    assert_eq!(exact.requests, vec![1, 1]);
    assert_eq!(exact.position, 1);
    assert_eq!(
        std::fs::read(root.join("exact")).expect("private file exists"),
        b"x"
    );

    let mut input = RecordingInput::new(b"xy".to_vec());
    let error = stage
        .write_file_new(Path::new("payload"), &mut input, 1)
        .expect_err("input exceeds limit");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(input.requests, vec![1, 1]);
    assert_eq!(input.returns, vec![1, 1]);
    assert_eq!(input.position, 2);
    assert!(!root.join("payload").exists());

    let mut short = RecordingInput::new(b"abc".to_vec()).with_short_read(1);
    stage
        .write_file_new(Path::new("short"), &mut short, 2)
        .expect("private stage handles short reads within quota");
    assert_eq!(short.requests, vec![2, 1, 1]);
    assert_eq!(short.returns, vec![1, 1, 0]);
    assert_eq!(short.position, 2);
    assert_eq!(
        std::fs::read(root.join("short")).expect("private file exists"),
        b"ab"
    );

    let mut interrupted_probe =
        RecordingInput::new(b"x".to_vec()).with_error_at(1, io::ErrorKind::Interrupted);
    stage
        .write_file_new(Path::new("interrupted-probe"), &mut interrupted_probe, 1)
        .expect("private probe retries Interrupted");
    assert_eq!(interrupted_probe.requests, vec![1, 1, 1]);
    assert_eq!(interrupted_probe.returns, vec![1, 0]);
    assert_eq!(interrupted_probe.position, 1);

    stage
        .cleanup()
        .expect("private stage cleanup should succeed");
}

#[test]
fn maximum_quota_does_not_overflow_for_small_input() {
    let (fixture, mut stage) = ordinary_stage();
    let mut input = RecordingInput::new(b"small".to_vec());
    stage
        .write_file_new(Path::new("payload"), &mut input, u64::MAX)
        .expect("small content fits maximum quota");
    assert_eq!(input.requests, vec![8192, 8192]);
    assert_eq!(input.position, 5);
    assert_eq!(
        std::fs::read(fixture.path().join(stage.relative_name()).join("payload"))
            .expect("file exists"),
        b"small"
    );
    stage.cleanup().expect("stage cleanup should succeed");
}

#[cfg(target_os = "linux")]
#[test]
fn owner_policy_stage_uses_limited_private_input_reads() {
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let options = LocalTempDirectoryOptions::new().with_owner_access_policy(policy);
    let (fixture, mut stage) = stage_with_options(options);
    let mut input = RecordingInput::new(b"xy".to_vec());
    let error = stage
        .write_file_new(Path::new("payload"), &mut input, 1)
        .expect_err("input exceeds limit");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(input.requests, vec![1, 1]);
    assert_eq!(input.returns, vec![2]);
    assert_eq!(input.position, 2);
    assert!(
        !fixture
            .path()
            .join(stage.relative_name())
            .join("payload")
            .exists()
    );
    stage
        .cleanup()
        .expect("owner-policy stage cleanup should succeed");
}
