// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalAtomicCommitError`.

use std::error::Error as _;
use std::io;

use crate::LocalAtomicCommitError;
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
use crate::LocalAtomicWriteStage;

fn create_test_error() -> LocalAtomicWriteError {
    LocalAtomicWriteError::new(
        LocalAtomicWriteStage::ReplaceDestination,
        "target".into(),
        None,
        LocalAtomicDestinationState::Unchanged,
        io::Error::other("boom"),
    )
}

#[test]
fn test_local_atomic_commit_error_retains_and_splits_recoverable_writer() {
    let mut commit = LocalAtomicCommitError::new(create_test_error(), Some(7_u8));
    assert_eq!(commit.error().kind(), io::ErrorKind::Other);
    assert_eq!(commit.writer(), Some(&7));
    assert_eq!(commit.writer_mut().map(|value| *value), Some(7));
    assert!(commit.to_string().contains("retained"));
    assert!(commit.source().is_some());
    let (error, writer) = commit.into_parts();
    assert_eq!(writer, Some(7));
    assert_eq!(error.kind(), io::ErrorKind::Other);
}

#[test]
fn test_local_atomic_commit_error_finalizes_or_returns_terminal_error() {
    let result = LocalAtomicCommitError::new(create_test_error(), Some(3_u8)).into_final_error_with(|writer, error| {
        assert_eq!(writer, 3);
        error
    });
    assert_eq!(result.kind(), io::ErrorKind::Other);
    let terminal = LocalAtomicCommitError::<u8>::new(create_test_error(), None);
    assert!(terminal.writer().is_none());
    assert!(terminal.to_string().contains("unavailable"));
    let _ = terminal.into_final_error_with(|_, error| error);
}
