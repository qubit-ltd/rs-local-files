// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Public construction and immutable configuration coverage.

use qubit_local_files::LocalCopyLimits;
use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileSystemBuilder;
use qubit_local_files::LocalFileSystemScope;
use qubit_local_files::LocalWalkLimits;

#[test]
fn cloned_filesystem_shares_authority_and_limits() {
    let root = tempfile::tempdir().expect("temporary root");
    let limits = LocalWalkLimits::new()
        .with_max_entries(32)
        .with_max_open_handles(4);
    let filesystem = LocalFileSystemBuilder::rooted(root.path())
        .walk_limits(limits)
        .build()
        .expect("rooted filesystem");

    let cloned = filesystem.clone();
    assert_eq!(cloned.scope(), LocalFileSystemScope::Rooted);
    assert_eq!(cloned.walk_limits(), limits);
}

#[test]
fn builder_rejects_zero_limits() {
    let error = LocalFileSystemBuilder::host()
        .copy_limits(LocalCopyLimits::new().with_max_bytes(0))
        .build()
        .expect_err("zero limits must be rejected");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
}
