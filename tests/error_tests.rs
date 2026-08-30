// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::io;
use std::path::Path;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileErrorSource;
use qubit_local_files::LocalFileOperation;
use qubit_local_files::LocalResourceKind;
use qubit_local_files::LocalResourceLimitError;

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
    let error = LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::OpenWriter);

    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.kind());
    assert_eq!(LocalFileOperation::OpenWriter, error.operation());
    assert!(error.source().is_none());
}

/// Verifies policy failures can expose a stable human-readable reason.
#[test]
fn test_local_file_error_reason_is_structured_and_displayed() {
    let error = LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::OpenWriter)
        .with_reason("required atomic publication is unavailable");

    assert_eq!(Some("required atomic publication is unavailable"), error.reason(),);
    assert!(error.to_string().contains("required atomic publication is unavailable"));
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
        (LocalFileErrorKind::AlreadyExists, io::ErrorKind::AlreadyExists),
        (LocalFileErrorKind::InvalidPath, io::ErrorKind::InvalidInput),
        (LocalFileErrorKind::InvalidOptions, io::ErrorKind::InvalidInput),
        (LocalFileErrorKind::InvalidState, io::ErrorKind::InvalidInput),
        (LocalFileErrorKind::NotDirectory, io::ErrorKind::NotADirectory),
        (LocalFileErrorKind::IsDirectory, io::ErrorKind::IsADirectory),
        (LocalFileErrorKind::NotFound, io::ErrorKind::NotFound),
        (LocalFileErrorKind::PermissionDenied, io::ErrorKind::PermissionDenied),
        (LocalFileErrorKind::ResourceLimit, io::ErrorKind::Other),
        (LocalFileErrorKind::DataCorruption, io::ErrorKind::InvalidData),
        (LocalFileErrorKind::RequirementNotMet, io::ErrorKind::Unsupported),
        (LocalFileErrorKind::Unsupported, io::ErrorKind::Unsupported),
        (LocalFileErrorKind::TypeConflict, io::ErrorKind::Other),
        (LocalFileErrorKind::PublicationIncomplete, io::ErrorKind::Other),
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
    assert_eq!(Some(io::ErrorKind::NotFound), source.io_error().map(io::Error::kind));
    assert_eq!(io::ErrorKind::NotFound, source.io_error_kind());
    assert!(source.into_source().is_some());
}

/// Verifies optional path context and source accessors preserve their
/// source-free semantics.
#[test]
fn test_local_file_error_exposes_optional_context_without_source() {
    let error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::OpenWriter)
        .with_path(Path::new("source").to_path_buf())
        .with_target(Path::new("target").to_path_buf());

    assert_eq!(Some(Path::new("source")), error.path());
    assert_eq!(Some(Path::new("target")), error.target());
    assert!(error.typed_source().is_none());
    assert!(Error::source(&error).is_none());
    assert!(error.into_source().is_none());
}

/// Verifies resource-limit errors preserve the complete budget facts and source
/// chain.
#[test]
fn test_local_resource_limit_error_preserves_budget_facts() {
    let source = LocalResourceLimitError::new(LocalResourceKind::OpenDirectory, 4, 0, 1);
    assert_eq!(LocalResourceKind::OpenDirectory, source.resource());
    assert_eq!(4, source.limit());
    assert_eq!(0, source.remaining());
    assert_eq!(1, source.requested());
    assert!(source.to_string().contains("open directory"));
    assert!(std::error::Error::source(&source).is_none());

    let error = LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::List)
        .with_path(Path::new("root/child").to_path_buf());
    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    assert!(error.resource_limit_error().is_none());
    assert_eq!(io::ErrorKind::Other, error.into_io_error().kind());
}

/// Verifies every resource kind has a stable diagnostic and that a resource
/// limit can participate in the typed local-error source chain.
#[test]
fn test_resource_limit_error_formats_all_resource_kinds_and_chains() {
    let cases = [
        (LocalResourceKind::Depth, "depth"),
        (LocalResourceKind::OpenDirectory, "open directory"),
        (LocalResourceKind::Entry, "entry"),
        (LocalResourceKind::SeenNameBytes, "seen-name bytes"),
        (LocalResourceKind::PathComponentBytes, "path-component bytes"),
        (LocalResourceKind::CopiedBytes, "copied bytes"),
    ];

    for (resource, expected_name) in cases {
        let error = LocalResourceLimitError::new(resource, 8, 2, 3);
        assert!(error.to_string().contains(expected_name));

        let source = LocalFileErrorSource::ResourceLimit(error);
        assert!(source.to_string().contains(expected_name));
        assert!(Error::source(&source).is_some());
    }
}

/// Verifies resource-limit accessors remain callable through their stable
/// public function signatures rather than only through inlined call sites.
#[test]
fn test_resource_limit_error_exposes_all_public_accessors() {
    let construct = std::hint::black_box(
        LocalResourceLimitError::new as fn(LocalResourceKind, usize, usize, usize) -> LocalResourceLimitError,
    );
    let resource =
        std::hint::black_box(LocalResourceLimitError::resource as fn(&LocalResourceLimitError) -> LocalResourceKind);
    let limit = std::hint::black_box(LocalResourceLimitError::limit as fn(&LocalResourceLimitError) -> usize);
    let remaining = std::hint::black_box(LocalResourceLimitError::remaining as fn(&LocalResourceLimitError) -> usize);
    let requested = std::hint::black_box(LocalResourceLimitError::requested as fn(&LocalResourceLimitError) -> usize);
    let error = construct(LocalResourceKind::CopiedBytes, 10, 4, 6);

    assert_eq!(LocalResourceKind::CopiedBytes, resource(&error));
    assert_eq!(10, limit(&error));
    assert_eq!(4, remaining(&error));
    assert_eq!(6, requested(&error));
}
