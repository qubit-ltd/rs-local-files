// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;

use qubit_local_files::LocalFileErrorSource;
use qubit_local_files::LocalPathCodecError;

/// Verifies path codec failures are typed error sources without a secondary
/// cause.
#[test]
fn test_path_codec_error_is_a_typed_error_without_a_source() {
    let error = LocalPathCodecError::InvalidEscape { offset: 3 };

    assert!(Error::source(&error).is_none());
    assert!(error.to_string().contains("3"));
}

/// Verifies the local error source retains a typed path codec failure.
#[test]
fn test_local_file_error_source_preserves_path_codec_error() {
    let source = LocalFileErrorSource::PathCodec(LocalPathCodecError::NonCanonicalText);

    assert!(matches!(
        source,
        LocalFileErrorSource::PathCodec(LocalPathCodecError::NonCanonicalText),
    ));
    assert!(Error::source(&source).is_some());
}
