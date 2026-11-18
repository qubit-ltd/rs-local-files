// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::Path;

use crate::LocalFileSystem;
use crate::LocalTempRecovery;
use crate::error::LocalFileEffectState;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileErrorSource;
use crate::error::LocalFileOperation;
use crate::error::LocalPathCodecError;
use crate::error::LocalReadLimitError;
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;

#[test]
fn test_recovery_source_forwards_nested_io_context() {
    let file = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
    let primary = LocalFileError::from_io(
        LocalFileOperation::Read,
        Some("read-path".into()),
        None,
        io::Error::from(io::ErrorKind::PermissionDenied),
    );
    let error = primary.with_temp_recovery(LocalTempRecovery::from_file(file));
    let source = error.into_source().unwrap();
    let LocalFileErrorSource::Recovery(nested) = source else {
        panic!("a recovery owner is represented by a recovery source");
    };

    assert_eq!(Some(LocalFileErrorKind::PermissionDenied), nested.cause_kind());
    assert_eq!(
        Some(io::ErrorKind::PermissionDenied),
        nested.io_error().map(io::Error::kind)
    );
    assert_eq!(io::ErrorKind::PermissionDenied, nested.io_error_kind());
    assert_eq!(Some(LocalFileErrorKind::PermissionDenied), nested.cause_kind());
}

#[test]
fn test_recovery_source_forwards_resource_and_read_limit_context() {
    let filesystem = LocalFileSystem::host().unwrap();
    let recovery = LocalTempRecovery::from_file(filesystem.create_temp_file().unwrap());
    let resource = LocalFileError::from_resource_limit(
        LocalFileOperation::Read,
        None,
        LocalResourceLimitError::new(LocalResourceKind::Entry, 8, 3, 5),
    )
    .with_temp_recovery(recovery);
    let source = resource.into_source().unwrap();
    let LocalFileErrorSource::Recovery(nested) = source else {
        panic!("a recovery owner is represented by a recovery source");
    };
    assert_eq!(Some(LocalFileErrorKind::ResourceLimit), nested.cause_kind());
    assert_eq!(io::ErrorKind::Other, nested.io_error_kind());
    assert_eq!(
        Some(5),
        nested.resource_limit_error().map(LocalResourceLimitError::requested)
    );

    let recovery = LocalTempRecovery::from_file(filesystem.create_temp_file().unwrap());
    let read = LocalFileError::from_read_limit(LocalFileOperation::Read, None, LocalReadLimitError::new(4, 7))
        .with_temp_recovery(recovery);
    let source = read.into_source().unwrap();
    let LocalFileErrorSource::Recovery(nested) = source else {
        panic!("a recovery owner is represented by a recovery source");
    };
    assert_eq!(Some(LocalFileErrorKind::ResourceLimit), nested.cause_kind());
    assert_eq!(io::ErrorKind::Other, nested.io_error_kind());
    assert_eq!(
        Some((4, 7)),
        nested.read_limit_error().map(|error| (error.limit(), error.observed()))
    );
}

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

#[test]
fn test_internal_error_remapping_retains_every_context_field() {
    let mut error = LocalFileError::from_path_codec(
        LocalFileOperation::Read,
        Some("original".into()),
        LocalPathCodecError::NativeNul,
    )
    .with_kind(LocalFileErrorKind::InvalidOptions)
    .with_operation(LocalFileOperation::OpenWriter);
    error.replace_paths(Some("source".into()), Some("target".into()), Some("/workspace".into()));

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
    assert_eq!(LocalFileOperation::OpenWriter, error.operation());
    assert_eq!(Some(Path::new("/workspace")), error.current_directory());
    assert_eq!(Some(Path::new("source")), error.path());
    assert_eq!(Some(Path::new("target")), error.target());
    assert_eq!(io::ErrorKind::InvalidInput, error.io_error_kind());
    assert!(error.typed_source().is_some());
    assert!(error.io_error().is_none());
    assert!(error.to_string().contains("caused by"));
}
