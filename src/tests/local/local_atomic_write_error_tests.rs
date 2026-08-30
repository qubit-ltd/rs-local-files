// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalAtomicWriteError`.

use std::error::Error as _;
use std::io;
use std::path::Path;

use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
use crate::LocalAtomicWriteStage;

fn create_test_error() -> LocalAtomicWriteError {
    LocalAtomicWriteError::new(
        LocalAtomicWriteStage::ReplaceDestination,
        "target".into(),
        Some("staging".into()),
        LocalAtomicDestinationState::Replaced,
        io::Error::other("boom"),
    )
}

#[test]
fn test_local_atomic_write_error_exposes_context_and_formats_secondary_errors() {
    let error = create_test_error()
        .with_cleanup_error(Some(io::Error::other("cleanup")))
        .with_parent_sync_error(Some(io::Error::other("sync")));
    assert_eq!(error.stage(), LocalAtomicWriteStage::ReplaceDestination);
    assert_eq!(error.path(), Path::new("target"));
    assert_eq!(error.temporary_path(), Some(Path::new("staging")));
    assert_eq!(error.destination_state(), LocalAtomicDestinationState::Replaced);
    assert!(error.cleanup_error().is_some());
    assert!(error.parent_sync_error().is_some());
    assert_eq!(error.source_error().kind(), io::ErrorKind::Other);
    assert_eq!(error.kind(), io::ErrorKind::Other);
    let display = error.to_string();
    assert!(display.contains("staging cleanup"));
    assert!(display.contains("parent synchronization"));
    assert!(error.source().is_some());
}

#[test]
fn test_local_atomic_write_error_splits_staging_parts_without_optional_context() {
    let (path, cleanup, source) = create_test_error().into_staging_parts();
    assert_eq!(path, Some("staging".into()));
    assert!(cleanup.is_none());
    assert_eq!(source.kind(), io::ErrorKind::Other);
    let no_staging = LocalAtomicWriteError::new(
        LocalAtomicWriteStage::PrepareParent,
        "target".into(),
        None,
        LocalAtomicDestinationState::Unchanged,
        io::Error::other("boom"),
    );
    assert!(!no_staging.to_string().contains("staging path"));
    let cleanup_only = create_test_error().with_cleanup_error(Some(io::Error::other("cleanup")));
    assert!(cleanup_only.to_string().contains("staging cleanup"));
    let parent_only = create_test_error().with_parent_sync_error(Some(io::Error::other("sync")));
    assert!(parent_only.to_string().contains("parent synchronization"));
    let plain = create_test_error()
        .with_cleanup_error(None)
        .with_parent_sync_error(None);
    assert!(plain.to_string().contains("atomic write"));
}
