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
