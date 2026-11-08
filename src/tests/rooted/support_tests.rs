// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Contract checks for rooted filesystem support operations.

use std::path::Path;

use crate::LocalFileOperation;
use crate::LocalSymlinkPolicy;
use crate::rooted::Root;
use crate::rooted_local_file_system::validate_rooted_list_start;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[test]
fn test_root_listing_start_native_metadata_failures_retain_input_context() {
    let fixture = tempfile::tempdir().expect("root fixture should exist");
    let root = Root::open(fixture.path()).expect("root authority should open");
    for path in [Path::new(""), Path::new(".")] {
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::RootMetadata, 1);
        let error = validate_rooted_list_start(&root, path, LocalSymlinkPolicy::FollowWithinScope, None)
            .expect_err("root metadata failure must stop listing startup");
        assert!(guard.fired());
        assert_eq!(error.operation(), LocalFileOperation::List);
        assert_eq!(error.path(), Some(Path::new("")));
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
    }
}
