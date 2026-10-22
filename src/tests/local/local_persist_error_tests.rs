// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalPersistError`.

use std::error::Error;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalPersistFailureState;
use crate::LocalPersistStage;
use crate::local::LocalPersistError;
use crate::outcome::LocalTempSourceState;

#[test]
fn test_persist_error_exposes_recoverable_context_and_resource() {
    let mut error = LocalPersistError::new(
        io::Error::from(io::ErrorKind::NotFound),
        String::from("temporary"),
        PathBuf::from("requested"),
        Some(PathBuf::from("/resolved")),
        LocalPersistStage::PrepareParent,
        LocalPersistFailureState::NotPublished,
        LocalTempSourceState::Owned,
    )
    .with_current_directory(PathBuf::from("/workspace"));

    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
    assert_eq!(LocalPersistStage::PrepareParent, error.stage());
    assert_eq!(LocalPersistFailureState::NotPublished, error.state());
    assert_eq!(Path::new("requested"), error.requested_target());
    assert_eq!(Some(Path::new("/resolved")), error.resolved_target());
    assert_eq!("temporary", error.resource());
    error.resource_mut().push_str("-updated");
    assert_eq!("temporary-updated", error.resource());
    assert!(error.to_string().contains("resolved as '/resolved'"));
    assert!(Error::source(&error).is_some());
    assert_eq!(Some(Path::new("/workspace")), error.error().current_directory());
}

#[test]
fn test_persist_error_parts_preserve_indeterminate_install_state() {
    let error = LocalPersistError::new(
        io::Error::from(io::ErrorKind::PermissionDenied),
        7_u8,
        PathBuf::from("requested"),
        None,
        LocalPersistStage::InstallDestination,
        LocalPersistFailureState::Indeterminate,
        LocalTempSourceState::Indeterminate,
    );

    let crate::local::LocalPersistErrorParts {
        error: source,
        resource,
        requested_target: requested,
        resolved_target: resolved,
        stage,
        state,
        ..
    } = error.into_parts();
    assert_eq!(io::ErrorKind::PermissionDenied, source.io_error_kind());
    assert_eq!(7, resource);
    assert_eq!(PathBuf::from("requested"), requested);
    assert_eq!(None, resolved);
    assert_eq!(LocalPersistStage::InstallDestination, stage);
    assert_eq!(LocalPersistFailureState::Indeterminate, state);
    assert_eq!(LocalFileOperation::PersistTemp, source.operation());
}

#[test]
fn test_persist_error_retains_explicit_durability_failure_states() {
    let source_error = LocalPersistError::new(
        io::Error::from(io::ErrorKind::Other),
        (),
        PathBuf::from("requested"),
        Some(PathBuf::from("/resolved")),
        LocalPersistStage::SynchronizeSource,
        LocalPersistFailureState::NotPublished,
        LocalTempSourceState::Owned,
    );
    assert_eq!(LocalPersistFailureState::NotPublished, source_error.state());

    let destination_error = LocalPersistError::new(
        io::Error::from(io::ErrorKind::Other),
        (),
        PathBuf::from("requested"),
        Some(PathBuf::from("/resolved")),
        LocalPersistStage::SynchronizeDestination,
        LocalPersistFailureState::Published,
        LocalTempSourceState::CleanupRequired,
    );
    assert_eq!(LocalPersistFailureState::Published, destination_error.state());
}
