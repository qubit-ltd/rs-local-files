// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{error::Error, io, path::Path};

use qubit_local_files::{LocalFileError, LocalFileErrorKind, LocalFileOperation};

/// Verifies that native I/O errors retain structured operation and path
/// context.
#[test]
fn test_local_file_error_from_io_preserves_context() {
    let error = LocalFileError::from_io(
        LocalFileOperation::Metadata,
        Some(Path::new("missing").to_path_buf()),
        None,
        io::Error::from(io::ErrorKind::NotFound),
    );

    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
    assert_eq!(LocalFileOperation::Metadata, error.operation());
    assert_eq!(Some(Path::new("missing")), error.path());
    assert_eq!(None, error.target());
    assert_eq!(
        Some(io::ErrorKind::NotFound),
        error
            .source()
            .and_then(|source| source.downcast_ref::<io::Error>())
            .map(io::Error::kind),
    );
}

/// Verifies that requirement failures do not masquerade as ordinary I/O errors.
#[test]
fn test_local_file_error_requirement_not_met_is_structured() {
    let error = LocalFileError::new(
        LocalFileErrorKind::RequirementNotMet,
        LocalFileOperation::OpenWriter,
    );

    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.kind());
    assert_eq!(LocalFileOperation::OpenWriter, error.operation());
    assert!(error.source().is_none());
}

/// Verifies policy failures can expose a stable human-readable reason.
#[test]
fn test_local_file_error_reason_is_structured_and_displayed() {
    let error = LocalFileError::new(
        LocalFileErrorKind::RequirementNotMet,
        LocalFileOperation::OpenWriter,
    )
    .with_reason("required atomic publication is unavailable");

    assert_eq!(
        Some("required atomic publication is unavailable"),
        error.reason(),
    );
    assert!(
        error
            .to_string()
            .contains("required atomic publication is unavailable")
    );
}

/// Verifies conversion to a standard I/O error preserves the native error kind.
#[test]
fn test_local_file_error_into_io_error_preserves_kind_and_context() {
    let error = LocalFileError::from_io(
        LocalFileOperation::Metadata,
        Some(Path::new("missing").to_path_buf()),
        None,
        io::Error::from(io::ErrorKind::NotFound),
    );

    let error = error.into_io_error();

    assert_eq!(io::ErrorKind::NotFound, error.kind());
    assert!(error.to_string().contains("Metadata failed"));
}

/// Verifies source-free classifications map to their nearest standard I/O
/// kinds and that typed sources can be consumed.
#[test]
fn test_local_file_error_adapts_source_free_kinds_and_consumes_source() {
    for (error_kind, io_kind) in [
        (
            LocalFileErrorKind::AlreadyExists,
            io::ErrorKind::AlreadyExists,
        ),
        (LocalFileErrorKind::InvalidPath, io::ErrorKind::InvalidInput),
        (
            LocalFileErrorKind::InvalidOptions,
            io::ErrorKind::InvalidInput,
        ),
        (
            LocalFileErrorKind::InvalidState,
            io::ErrorKind::InvalidInput,
        ),
        (
            LocalFileErrorKind::NotDirectory,
            io::ErrorKind::NotADirectory,
        ),
        (LocalFileErrorKind::IsDirectory, io::ErrorKind::IsADirectory),
        (LocalFileErrorKind::NotFound, io::ErrorKind::NotFound),
        (
            LocalFileErrorKind::PermissionDenied,
            io::ErrorKind::PermissionDenied,
        ),
        (
            LocalFileErrorKind::ResourceLimit,
            io::ErrorKind::StorageFull,
        ),
        (
            LocalFileErrorKind::DataCorruption,
            io::ErrorKind::InvalidData,
        ),
        (
            LocalFileErrorKind::RequirementNotMet,
            io::ErrorKind::Unsupported,
        ),
        (LocalFileErrorKind::Unsupported, io::ErrorKind::Unsupported),
        (LocalFileErrorKind::TypeConflict, io::ErrorKind::Other),
        (
            LocalFileErrorKind::PublicationIncomplete,
            io::ErrorKind::Other,
        ),
        (LocalFileErrorKind::Indeterminate, io::ErrorKind::Other),
        (LocalFileErrorKind::Io, io::ErrorKind::Other),
    ] {
        assert_eq!(
            io_kind,
            LocalFileError::new(error_kind, LocalFileOperation::Metadata)
                .into_io_error()
                .kind(),
        );
    }

    let source = LocalFileError::from_io(
        LocalFileOperation::OpenReader,
        None,
        None,
        io::Error::from(io::ErrorKind::NotFound),
    );
    assert_eq!(
        Some(io::ErrorKind::NotFound),
        source.io_error().map(io::Error::kind)
    );
    assert_eq!(io::ErrorKind::NotFound, source.io_error_kind());
    assert!(source.into_source().is_some());
}

/// Verifies optional path context and source accessors preserve their
/// source-free semantics.
#[test]
fn test_local_file_error_exposes_optional_context_without_source() {
    let error = LocalFileError::new(
        LocalFileErrorKind::InvalidOptions,
        LocalFileOperation::OpenWriter,
    )
    .with_path(Path::new("source").to_path_buf())
    .with_target(Path::new("target").to_path_buf());

    assert_eq!(Some(Path::new("source")), error.path());
    assert_eq!(Some(Path::new("target")), error.target());
    assert!(error.typed_source().is_none());
    assert!(Error::source(&error).is_none());
    assert!(error.into_source().is_none());
}
