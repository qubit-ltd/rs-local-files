// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalCopyDirError`.

use std::error::Error as _;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalCopyDirError;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;

#[test]
fn test_local_copy_dir_error_exposes_parts_and_formats_cleanup_context() {
    let error = LocalCopyDirError::new(
        LocalCopyDirStage::CopyFileContents,
        "source".into(),
        "destination".into(),
        LocalCopyDirStats::default(),
        io::Error::other("copy failed"),
    )
    .with_staging_context(
        "staging".into(),
        Some(io::Error::other("cleanup failed")),
    );
    assert_eq!(error.stage(), LocalCopyDirStage::CopyFileContents);
    assert_eq!(error.source_path(), Path::new("source"));
    assert_eq!(error.destination_path(), Path::new("destination"));
    assert_eq!(error.stats(), &LocalCopyDirStats::default());
    assert_eq!(error.temporary_path(), Some(Path::new("staging")));
    assert!(error.cleanup_error().is_some());
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.error().kind(), io::ErrorKind::Other);
    assert!(error.to_string().contains("staging cleanup"));
    assert!(error.source().is_some());
    let (_, source, destination, _, staging, cleanup, native) =
        error.into_parts();
    assert_eq!(source, PathBuf::from("source"));
    assert_eq!(destination, PathBuf::from("destination"));
    assert_eq!(
        staging.expect("staging path").as_ref(),
        Path::new("staging")
    );
    assert!(cleanup.is_some());
    assert_eq!(native.kind(), io::ErrorKind::Other);
}
