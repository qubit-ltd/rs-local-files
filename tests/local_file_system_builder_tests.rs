// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Public construction and instance-configuration coverage.

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::path::LocalFileSystemScope;

#[test]
fn cloned_filesystem_shares_authority_but_copies_configuration() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut filesystem = LocalFileSystem::rooted(root.path()).expect("rooted filesystem");
    filesystem
        .set_default_list_options(
            LocalListOptions::new()
                .with_max_entries(32)
                .with_max_open_directories(4),
        )
        .expect("list defaults should be accepted");

    let mut cloned = filesystem.clone();
    assert_eq!(cloned.scope(), LocalFileSystemScope::Rooted);
    assert_eq!(cloned.default_list_options().max_entries(), Some(32));
    cloned
        .set_default_list_options(LocalListOptions::new())
        .expect("clone defaults should remain configurable");
    assert_eq!(filesystem.default_list_options().max_entries(), Some(32));
    assert_eq!(cloned.default_list_options().max_entries(), None);
}

#[test]
fn configuration_setters_reject_invalid_options_transactionally() {
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let original = *filesystem.default_list_options();
    let error = filesystem
        .set_default_list_options(LocalListOptions::new().with_max_open_directories(0))
        .expect_err("zero open-directory budgets must be rejected");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(filesystem.default_list_options(), &original);
}
