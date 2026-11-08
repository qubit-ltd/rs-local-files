// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local tests for rooted path support contracts.

use std::path::Path;

use tempfile::tempdir;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::rooted::Root;
use crate::rooted_local_file_system::temp_candidate;
use crate::rooted_local_file_system::validate_rooted_temp_parent;

#[test]
fn test_rooted_temp_parent_rejects_parent_traversal_with_original_context() {
    let fixture = tempdir().expect("root fixture should exist");
    let root = Root::open(fixture.path()).expect("root authority should open");
    let path = Path::new("../outside");

    let error = validate_rooted_temp_parent(&root, path, LocalFileOperation::Metadata)
        .expect_err("temporary parent must stay within root");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(path));
}

#[test]
fn test_rooted_temp_candidate_classifies_invalid_affix_as_invalid_options() {
    let parent = Path::new("temp-parent");
    for (prefix, suffix) in [(Some("bad/prefix"), None), (None, Some("bad/suffix"))] {
        let error = temp_candidate(parent, prefix, suffix, LocalFileOperation::Metadata)
            .expect_err("an affix may not introduce additional path components");

        assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.path(), Some(parent));
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::InvalidInput);
    }
}
