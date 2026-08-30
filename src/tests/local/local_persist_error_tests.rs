// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalPersistError`.

use std::error::Error as _;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileErrorKind;
use crate::LocalPersistError;
use crate::LocalPersistFailureState;
use crate::LocalPersistStage;

#[test]
fn test_local_persist_error_exposes_recoverable_context_and_parts() {
    let mut error = LocalPersistError::new(
        io::Error::new(io::ErrorKind::NotFound, "missing"),
        String::from("resource"),
        "requested".into(),
        Some("resolved".into()),
        LocalPersistStage::PrepareParent,
    );
    assert_eq!(error.resource(), "resource");
    error.resource_mut().push('!');
    assert_eq!(error.resource(), "resource!");
    assert_eq!(error.requested_target(), Path::new("requested"));
    assert_eq!(error.resolved_target(), Some(Path::new("resolved")));
    assert_eq!(error.stage(), LocalPersistStage::PrepareParent);
    assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
    assert_eq!(error.kind(), LocalFileErrorKind::NotFound);
    assert!(error.to_string().contains("resolved as 'resolved'"));
    assert!(error.source().is_some());
    let (error, resource, requested, resolved, stage, state) = error.into_parts_with_state();
    assert_eq!(resource, "resource!");
    assert_eq!(requested, PathBuf::from("requested"));
    assert_eq!(resolved, Some(PathBuf::from("resolved")));
    assert_eq!(stage, LocalPersistStage::PrepareParent);
    assert_eq!(state, LocalPersistFailureState::NotPublished);
    assert_eq!(error.kind(), LocalFileErrorKind::NotFound);

    let error = LocalPersistError::new(
        io::Error::other("unknown"),
        (),
        "requested".into(),
        None,
        LocalPersistStage::InstallDestination,
    );
    assert!(error.to_string().contains("requested target 'requested'"));
    assert_eq!(error.state(), LocalPersistFailureState::Indeterminate);
    let _ = error.into_parts();
}
