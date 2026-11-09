// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
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
//! operations execute. Native and rooted namespace paths are tracked separately.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use qubit_io::Output;
use qubit_io::Streams;
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

#[derive(Default, Debug)]
pub(crate) struct ConditionalWitness {
    pub successful_commit: usize,
    pub absent_conflict: usize,
    pub content_mismatch: usize,
    pub abort: usize,
    pub drop: usize,
    pub race: usize,
    pub hook_io_error: usize,
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
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .ok()?;
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = None;
    let mut candidate = Vec::new();
    let mut before_writer: Option<Option<Vec<u8>>> = None;
    let mut witness = ConditionalWitness::default();
    let mut pending_race = false;
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
                    let _ = fs::remove_file(&native_target);
                } else {
                    let _ = fs::write(&native_target, payload);
                }
            }
            1 | 2 | 7 | 8 | 9 => {
                if writer.is_some() {
                    continue;
                }
                before_writer = Some(fs::read(&native_target).ok());
                let precondition = if opcode == 2 || opcode == 9 {
                    let current = fs::read(&native_target).unwrap_or_default();
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
                    let bytes = if opcode == 7 {
                        payload.to_vec()
                    } else {
                        Vec::new()
                    };
                    let action = move || {
                        if opcode == 7 {
                            fs::write(path, bytes)
                        } else {
                            Err(std::io::Error::other("injected conditional model error"))
                        }
                    };
                    let operation_filesystem = filesystem
                        .clone()
                        .with_conditional_test_hooks(TestConditionalHooks::once(point, action));
                    pending_race = opcode == 7;
                    race_sentinel = (opcode == 7).then(|| payload.to_vec());
                    candidate.clear();
                    match operation_filesystem.open_conditional_writer::<Sha256>(
                        namespace_target,
                        precondition,
                        &options,
                    ) {
                        Ok(opened) => writer = Some(opened),
                        Err(error)
                            if error.kind()
                                == qubit_local_files::error::LocalFileErrorKind::Unsupported =>
                        {
                            witness.unsupported += 1
                        }
                        Err(_) => {}
                    }
                    continue;
                }
                candidate.clear();
                match filesystem.open_conditional_writer::<Sha256>(
                    namespace_target,
                    precondition,
                    &options,
                ) {
                    Ok(opened) => writer = Some(opened),
                    Err(error)
                        if error.kind()
                            == qubit_local_files::error::LocalFileErrorKind::Unsupported =>
                    {
                        witness.unsupported += 1
                    }
                    Err(_) => {}
                }
            }
            3 => {
                if let Some(opened) = writer.as_mut() {
                    let bytes = &payload[..payload
                        .len()
                        .min(MAX_PAYLOAD.saturating_sub(candidate.len()))];
                    if opened.write_fully(bytes).is_ok() {
                        candidate.extend_from_slice(bytes);
                    }
                }
            }
            4 => {
                if let Some(opened) = writer.take() {
                    match opened.commit() {
                        Ok(saved) => {
                            witness.successful_commit += 1;
                            assert_eq!(saved.digest(), &Sha256::digest(&candidate));
                            assert_eq!(
                                fs::read(&native_target).expect("published target readable"),
                                candidate
                            );
                            if !saved.cleanup_errors().is_empty() {
                                witness.cleanup_error += 1;
                            }
                        }
                        Err(error) => {
                            if pending_race
                                && error.publication()
                                    == LocalConditionalPublicationState::NotPublished
                            {
                                witness.race += 1;
                                assert_eq!(
                                    fs::read(&native_target)
                                        .expect("race sentinel remains readable"),
                                    race_sentinel.take().expect("race sentinel was captured"),
                                );
                            }
                            pending_race = false;
                            if error.publication() == LocalConditionalPublicationState::NotPublished
                            {
                                if error.reason() == qubit_local_files::error::LocalConditionalFailureReason::TargetExists { witness.absent_conflict += 1; }
                                if error.reason() == qubit_local_files::error::LocalConditionalFailureReason::PreconditionMismatch { witness.content_mismatch += 1; }
                            }
                            if error.reason()
                                == qubit_local_files::error::LocalConditionalFailureReason::Io
                            {
                                witness.hook_io_error += 1;
                            }
                            if error.candidate().is_some_and(|file| {
                                file.role() == LocalRecoveryFileRole::CandidateCopy
                            }) {
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
                    assert_eq!(
                        fs::read(&native_target).ok(),
                        before_writer.take().expect("writer snapshot exists")
                    );
                    pending_race = false;
                    witness.abort += 1;
                }
            }
            6 => {
                if writer.take().is_some() {
                    assert_eq!(
                        fs::read(&native_target).ok(),
                        before_writer.take().expect("writer snapshot exists")
                    );
                    pending_race = false;
                    witness.drop += 1;
                }
            }
            _ => unreachable!(),
        }
    }
    drop(writer);
    Some(witness)
}

pub(crate) fn regression_sequences() -> [(&'static str, &'static [u8]); 8] {
    [
        ("absent commit", &[0, 0, 1, 0, 3, 1, b'B', 4, 0]),
        (
            "absent conflict retains candidate",
            &[0, 1, b'A', 1, 0, 3, 1, b'B', 4, 0],
        ),
        (
            "abort and drop",
            &[1, 0, 3, 1, b'X', 5, 0, 1, 0, 3, 1, b'Y', 6, 0],
        ),
        ("content success", &[0, 1, b'A', 2, 0, 3, 1, b'B', 4, 0]),
        (
            "content mismatch",
            &[0, 1, b'A', 2, 0, 0, 1, b'C', 3, 1, b'B', 4, 0],
        ),
        ("publication race", &[7, 1, b'R', 3, 1, b'B', 4, 0]),
        ("hook I/O error", &[8, 0, 3, 1, b'B', 4, 0]),
        ("cleanup error", &[0, 1, b'A', 9, 0, 3, 1, b'B', 4, 0]),
    ]
}

pub(crate) fn assert_regression(name: &str, witness: &ConditionalWitness) {
    match name {
        "absent commit" => assert!(witness.successful_commit > 0, "{witness:?}"),
        "absent conflict retains candidate" => assert!(
            witness.absent_conflict > 0 && witness.retained_candidate > 0,
            "{witness:?}"
        ),
        "abort and drop" => assert!(witness.abort > 0 && witness.drop > 0, "{witness:?}"),
        "content success" => assert!(witness.successful_commit > 0, "{witness:?}"),
        "content mismatch" => assert!(witness.content_mismatch > 0, "{witness:?}"),
        "publication race" => assert!(
            witness.race > 0 && witness.absent_conflict > 0,
            "{witness:?}"
        ),
        "hook I/O error" => assert!(witness.hook_io_error > 0, "{witness:?}"),
        "cleanup error" => assert!(
            witness.successful_commit > 0 && witness.cleanup_error > 0,
            "{witness:?}"
        ),
        _ => panic!("unknown regression sequence {name}"),
    }
}
