// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded conditional-write model shared by fuzz and ordinary tests.
//!
//! Each operation starts with an opcode and a one-byte payload length. The
//! payload is truncated to 256 bytes and consumes that many input bytes (or
//! all remaining bytes when input is short). Opcodes: 0 writes initial target;
//! 1 opens with Absent; 2 opens with Content(target digest); 3 writes payload;
//! 4 commits; 5 aborts; 6 drops; 7 installs a pre-publication race; 8 injects a
//! pre-publication I/O error; 9 injects a cleanup error after a Content commit.
//! Returned recovery files are checked immediately. At most 32
//! operations execute. Native and rooted namespace paths are tracked
//! separately.
#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use qubit_io::Output;
use qubit_io::Streams;
use qubit_local_files::LocalConditionalWriter;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::outcome::LocalConditionalPublicationState;
use qubit_local_files::policy::LocalSymlinkPolicy;
use qubit_local_files::recovery::LocalRecoveryFileRole;
use qubit_local_files::test_support::TestConditionalHooks;
use qubit_local_files::test_support::TestConditionalPoint;
use sha2::Digest;
use sha2::Sha256;

use crate::support::FuzzRoot;

const MAX_OPERATIONS: usize = 32;
const MAX_PAYLOAD: usize = 256;

fn read_optional(path: &Path) -> Option<Vec<u8>> {
    match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("read model target: {error}"),
    }
}

/// Whether this target, scope, and condition has documented support. Linux
/// and macOS claim both scopes and conditions; Windows excludes Rooted Content;
/// other platforms have no claimed conditional backend.
fn unsupported_is_allowed(rooted: bool, content: bool) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let _ = (rooted, content);
        false
    }
    #[cfg(windows)]
    {
        rooted && content
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (rooted, content);
        true
    }
}

fn require_open(
    result: qubit_local_files::LocalResult<LocalConditionalWriter<Sha256>>,
    unsupported_allowed: bool,
    witness: &mut ConditionalWitness,
) -> Option<LocalConditionalWriter<Sha256>> {
    match result {
        Ok(opened) => Some(opened),
        Err(error)
            if error.kind() == qubit_local_files::error::LocalFileErrorKind::Unsupported && unsupported_allowed =>
        {
            witness.unsupported += 1;
            None
        }
        Err(error) => panic!("unexpected conditional writer open failure: {error}"),
    }
}

#[derive(Default, Debug)]
pub(crate) struct ConditionalWitness {
    pub successful_commit: usize,
    pub absent_conflict: usize,
    pub content_mismatch: usize,
    pub abort: usize,
    pub drop: usize,
    pub race: usize,
    pub hook_io_error: usize,
    pub ordinary_commit_io_error: usize,
    pub cleanup_error: usize,
    pub retained_candidate: usize,
    pub unsupported: usize,
}

pub(crate) fn run_model(data: &[u8], rooted: bool) -> Option<ConditionalWitness> {
    let root = FuzzRoot::create(if rooted {
        "conditional-rooted"
    } else {
        "conditional-host"
    })?;
    let native_target = root.path().join("target");
    let namespace_target = if rooted {
        Path::new("/target")
    } else {
        native_target.as_path()
    };
    let mut filesystem = if rooted {
        LocalFileSystem::rooted(root.path()).ok()?
    } else {
        LocalFileSystem::host().ok()?
    };
    filesystem.set_symlink_policy(LocalSymlinkPolicy::Reject).ok()?;
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = None;
    let mut candidate = Vec::new();
    let mut expected_target: Option<Vec<u8>> = None;
    let mut target_known = true;
    let mut witness = ConditionalWitness::default();
    let mut pending_race = false;
    let mut pending_hook_io_error = false;
    let mut pending_hook_invoked: Option<Arc<AtomicBool>> = None;
    let mut race_sentinel: Option<Vec<u8>> = None;
    let mut cursor = 0;

    for _ in 0..MAX_OPERATIONS {
        if cursor + 2 > data.len() {
            break;
        }
        let opcode = data[cursor] % 10;
        let length = usize::from(data[cursor + 1]).min(MAX_PAYLOAD);
        cursor += 2;
        let end = cursor.saturating_add(length).min(data.len());
        let payload = &data[cursor..end];
        cursor = end;
        match opcode {
            0 => {
                if payload.is_empty() {
                    if let Err(error) = fs::remove_file(&native_target) {
                        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                    }
                    expected_target = None;
                } else {
                    fs::write(&native_target, payload).expect("write model target");
                    expected_target = Some(payload.to_vec());
                }
                target_known = true;
                assert_eq!(read_optional(&native_target), expected_target);
            }
            1 | 2 | 7 | 8 | 9 => {
                if writer.is_some() {
                    continue;
                }
                if (opcode == 2 || opcode == 9) && !target_known {
                    continue;
                }
                let precondition = if opcode == 2 || opcode == 9 {
                    let current = expected_target.clone().unwrap_or_default();
                    LocalWritePrecondition::Content(Sha256::digest(current))
                } else {
                    LocalWritePrecondition::Absent
                };
                if opcode == 7 || opcode == 8 || opcode == 9 {
                    let point = if opcode == 9 {
                        TestConditionalPoint::BeforeCleanupOld
                    } else {
                        TestConditionalPoint::BeforePublish
                    };
                    let path = native_target.clone();
                    let bytes = if opcode == 7 { payload.to_vec() } else { Vec::new() };
                    let hook_invoked = Arc::new(AtomicBool::new(false));
                    let hook_invoked_by_action = Arc::clone(&hook_invoked);
                    let action = move || {
                        hook_invoked_by_action.store(true, Ordering::SeqCst);
                        if opcode == 7 {
                            fs::write(path, bytes)
                        } else {
                            Err(std::io::Error::other("injected conditional model error"))
                        }
                    };
                    let operation_filesystem = filesystem
                        .clone()
                        .with_conditional_test_hooks(TestConditionalHooks::once(point, action));
                    candidate.clear();
                    let content = opcode == 2 || opcode == 9;
                    let opened = require_open(
                        operation_filesystem.open_conditional_writer::<Sha256>(
                            namespace_target,
                            precondition,
                            &options,
                        ),
                        unsupported_is_allowed(rooted, content),
                        &mut witness,
                    );
                    pending_race = opcode == 7 && opened.is_some();
                    pending_hook_io_error = opcode == 8 && opened.is_some();
                    pending_hook_invoked = opened.as_ref().map(|_| hook_invoked);
                    race_sentinel = pending_race.then(|| payload.to_vec());
                    writer = opened;
                    continue;
                }
                candidate.clear();
                let content = opcode == 2 || opcode == 9;
                writer = require_open(
                    filesystem.open_conditional_writer::<Sha256>(namespace_target, precondition, &options),
                    unsupported_is_allowed(rooted, content),
                    &mut witness,
                );
            }
            3 => {
                if let Some(opened) = writer.as_mut() {
                    let bytes = &payload[..payload.len().min(MAX_PAYLOAD.saturating_sub(candidate.len()))];
                    opened
                        .write_fully(bytes)
                        .unwrap_or_else(|error| panic!("unexpected conditional candidate write failure: {error}"));
                    candidate.extend_from_slice(bytes);
                }
            }
            4 => {
                if let Some(opened) = writer.take() {
                    match opened.commit() {
                        Ok(saved) => {
                            let hook_invoked = pending_hook_invoked
                                .as_ref()
                                .is_some_and(|flag| flag.load(Ordering::SeqCst));
                            assert!(
                                !(pending_hook_io_error && hook_invoked),
                                "injected hook I/O error was ignored"
                            );
                            assert!(
                                !(pending_race && hook_invoked),
                                "publication race unexpectedly succeeded"
                            );
                            witness.successful_commit += 1;
                            assert_eq!(saved.digest(), &Sha256::digest(&candidate));
                            expected_target = Some(candidate.clone());
                            target_known = true;
                            assert_eq!(read_optional(&native_target), expected_target);
                            if !saved.cleanup_errors().is_empty() {
                                witness.cleanup_error += 1;
                            }
                            pending_race = false;
                            pending_hook_io_error = false;
                            pending_hook_invoked = None;
                        }
                        Err(error) => {
                            let hook_invoked = pending_hook_invoked
                                .as_ref()
                                .is_some_and(|flag| flag.load(Ordering::SeqCst));
                            let was_race = pending_race && hook_invoked;
                            if was_race {
                                assert_eq!(error.publication(), LocalConditionalPublicationState::NotPublished);
                                assert_eq!(
                                    error.reason(),
                                    qubit_local_files::error::LocalConditionalFailureReason::TargetExists
                                );
                                witness.race += 1;
                                expected_target = Some(race_sentinel.take().expect("race sentinel was captured"));
                                target_known = true;
                            }
                            pending_race = false;
                            match error.publication() {
                                LocalConditionalPublicationState::NotPublished => {
                                    if target_known {
                                        assert_eq!(read_optional(&native_target), expected_target);
                                    }
                                }
                                LocalConditionalPublicationState::Published => {
                                    expected_target = Some(candidate.clone());
                                    target_known = true;
                                    assert_eq!(read_optional(&native_target), expected_target);
                                }
                                LocalConditionalPublicationState::Restored => {
                                    if target_known {
                                        assert_eq!(read_optional(&native_target), expected_target);
                                    }
                                }
                                LocalConditionalPublicationState::Indeterminate => {
                                    target_known = false;
                                }
                                _ => unreachable!("publication state handled"),
                            }
                            if error.publication() == LocalConditionalPublicationState::NotPublished {
                                if error.reason()
                                    == qubit_local_files::error::LocalConditionalFailureReason::TargetExists
                                {
                                    witness.absent_conflict += 1;
                                }
                                if error.reason()
                                    == qubit_local_files::error::LocalConditionalFailureReason::PreconditionMismatch
                                {
                                    witness.content_mismatch += 1;
                                }
                            }
                            if pending_hook_io_error && hook_invoked {
                                assert_eq!(
                                    error.reason(),
                                    qubit_local_files::error::LocalConditionalFailureReason::Io
                                );
                                assert_eq!(error.error().io_error_kind(), std::io::ErrorKind::Other);
                                witness.hook_io_error += 1;
                            } else if error.reason() == qubit_local_files::error::LocalConditionalFailureReason::Io {
                                witness.ordinary_commit_io_error += 1;
                            }
                            pending_hook_io_error = false;
                            pending_hook_invoked = None;
                            if error
                                .candidate()
                                .is_some_and(|file| file.role() == LocalRecoveryFileRole::CandidateCopy)
                            {
                                witness.retained_candidate += 1;
                                let mut reader = error
                                    .candidate()
                                    .unwrap()
                                    .open_reader()
                                    .expect("candidate recovery reader");
                                let mut bytes = Vec::new();
                                Streams::copy_input_to_output(&mut reader, &mut bytes)
                                    .expect("candidate recovery bytes");
                                assert_eq!(bytes, candidate);
                            }
                        }
                    }
                }
            }
            5 => {
                if let Some(mut opened) = writer.take() {
                    let _ = opened.abort().expect("abort candidate");
                    if target_known {
                        assert_eq!(read_optional(&native_target), expected_target);
                    }
                    pending_race = false;
                    pending_hook_io_error = false;
                    pending_hook_invoked = None;
                    witness.abort += 1;
                }
            }
            6 => {
                if writer.take().is_some() {
                    if target_known {
                        assert_eq!(read_optional(&native_target), expected_target);
                    }
                    pending_race = false;
                    pending_hook_io_error = false;
                    pending_hook_invoked = None;
                    witness.drop += 1;
                }
            }
            _ => unreachable!(),
        }
    }
    drop(writer);
    if target_known {
        assert_eq!(read_optional(&native_target), expected_target);
    }
    Some(witness)
}

pub(crate) fn regression_sequences() -> [(&'static str, &'static [u8]); 10] {
    [
        ("absent commit", &[0, 0, 1, 0, 3, 1, b'B', 4, 0]),
        (
            "absent conflict retains candidate",
            &[0, 1, b'A', 1, 0, 3, 1, b'B', 4, 0],
        ),
        ("abort and drop", &[1, 0, 3, 1, b'X', 5, 0, 1, 0, 3, 1, b'Y', 6, 0]),
        (
            "external mutation then abort",
            &[0, 1, b'A', 2, 0, 3, 1, b'B', 0, 1, b'C', 5, 0],
        ),
        (
            "precondition blocks race hook",
            &[0, 1, 5, 47, 0, 0, 4, 184, 0, 0, 4, 184, 0, 0, 5, 0],
        ),
        ("content success", &[0, 1, b'A', 2, 0, 3, 1, b'B', 4, 0]),
        ("content mismatch", &[0, 1, b'A', 2, 0, 0, 1, b'C', 3, 1, b'B', 4, 0]),
        ("publication race", &[7, 1, b'R', 3, 1, b'B', 4, 0]),
        ("hook I/O error", &[8, 0, 3, 1, b'B', 4, 0]),
        ("cleanup error", &[0, 1, b'A', 9, 0, 3, 1, b'B', 4, 0]),
    ]
}

pub(crate) fn assert_regression(name: &str, witness: &ConditionalWitness, rooted: bool) {
    let content = matches!(name, "content success" | "content mismatch" | "cleanup error");
    if unsupported_is_allowed(rooted, content) && witness.unsupported > 0 {
        return;
    }
    match name {
        "absent commit" => assert!(witness.successful_commit > 0, "{witness:?}"),
        "absent conflict retains candidate" => assert!(
            witness.absent_conflict > 0 && witness.retained_candidate > 0,
            "{witness:?}"
        ),
        "abort and drop" => assert!(witness.abort > 0 && witness.drop > 0, "{witness:?}"),
        "external mutation then abort" => assert!(witness.abort > 0, "{witness:?}"),
        "precondition blocks race hook" => assert!(witness.absent_conflict > 0 && witness.race == 0, "{witness:?}"),
        "content success" => assert!(witness.successful_commit > 0, "{witness:?}"),
        "content mismatch" => assert!(witness.content_mismatch > 0, "{witness:?}"),
        "publication race" => assert!(witness.race > 0 && witness.absent_conflict > 0, "{witness:?}"),
        "hook I/O error" => assert!(witness.hook_io_error > 0, "{witness:?}"),
        "cleanup error" => assert!(
            witness.successful_commit > 0 && witness.cleanup_error > 0,
            "{witness:?}"
        ),
        _ => panic!("unknown regression sequence {name}"),
    }
}
