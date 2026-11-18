// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;

use crate::LocalFileSystem;
use crate::error::LocalFileEffectState;
use crate::options::LocalDeleteOptions;

fn assert_budget_rejection_preserves_tree(options: LocalDeleteOptions) {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let root = fixture.path().join("tree");
    fs::create_dir_all(root.join("branch")).expect("nested directories should exist");
    fs::write(root.join("branch/leaf"), b"keep me").expect("leaf should exist");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");

    let error = filesystem
        .delete_directory_with_options(&root, &options.with_recursive())
        .expect_err("configured deletion budget should reject this tree");

    assert!(error.resource_limit_error().is_some());
    assert_eq!(error.effect_state(), Some(LocalFileEffectState::Unchanged));
    assert_eq!(fs::read(root.join("branch/leaf")).unwrap(), b"keep me");
    assert!(root.join("branch").is_dir());
}

#[test]
fn test_entry_budget_rejects_tree_before_removing_any_entry() {
    assert_budget_rejection_preserves_tree(LocalDeleteOptions::new().with_max_entries(1));
}

#[test]
fn test_depth_budget_rejects_deep_entry_before_removing_any_entry() {
    assert_budget_rejection_preserves_tree(LocalDeleteOptions::new().with_max_depth(1));
}

#[test]
fn test_pending_path_budget_rejects_root_before_filesystem_mutation() {
    assert_budget_rejection_preserves_tree(LocalDeleteOptions::new().with_max_pending_path_bytes(0));
}
