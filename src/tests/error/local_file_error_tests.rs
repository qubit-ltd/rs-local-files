// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

use crate::error::LocalFileEffectState;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::error::LocalPathCodecError;
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;

#[test]
fn test_effect_queries_preserve_cause_and_effect_for_reclassified_io() {
    let error = LocalFileError::from_io(
        LocalFileOperation::DeleteDirectory,
        None,
        None,
        io::Error::from(io::ErrorKind::PermissionDenied),
    )
    .with_kind(LocalFileErrorKind::PublicationIncomplete);

    assert_eq!(Some(LocalFileErrorKind::PermissionDenied), error.cause_kind());
    assert_eq!(Some(LocalFileEffectState::PartiallyApplied), error.effect_state());
    assert_eq!(io::ErrorKind::PermissionDenied, error.io_error_kind());
    assert!(error.io_error().is_some());
}

#[test]
fn test_effect_queries_preserve_resource_limit_cause() {
    let source = LocalResourceLimitError::new(LocalResourceKind::Entry, 4, 1, 2);
    let error = LocalFileError::from_resource_limit(LocalFileOperation::Copy, None, source)
        .with_kind(LocalFileErrorKind::PublicationIncomplete);

    assert_eq!(Some(LocalFileErrorKind::ResourceLimit), error.cause_kind());
    assert_eq!(Some(LocalFileEffectState::PartiallyApplied), error.effect_state());
    assert!(error.resource_limit_error().is_some());
}

#[test]
fn test_effect_queries_map_path_codec_cause() {
    let error = LocalFileError::from_path_codec(
        LocalFileOperation::Read,
        Some("source".into()),
        LocalPathCodecError::NativeNul,
    )
    .with_kind(LocalFileErrorKind::Indeterminate);

    assert_eq!(Some(LocalFileErrorKind::InvalidPath), error.cause_kind());
    assert_eq!(Some(LocalFileEffectState::Indeterminate), error.effect_state());
}

#[test]
fn test_effect_queries_preserve_semantic_cause_reclassification() {
    let error = LocalFileError::from_io(
        LocalFileOperation::Copy,
        None,
        None,
        io::Error::from(io::ErrorKind::InvalidInput),
    )
    .with_kind(LocalFileErrorKind::TypeConflict);

    assert_eq!(Some(LocalFileErrorKind::TypeConflict), error.cause_kind());
    assert_eq!(None, error.effect_state());
}
