// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
#[test]
fn test_review_tree_sync_orders_files_directories_root_and_parent() {
    crate::directory::LocalStagedDirectory::test_review_tree_sync_orders_files_directories_root_and_parent();
}

#[cfg(unix)]
#[test]
fn test_review_tree_sync_stops_after_nested_directory_failure() {
    crate::directory::LocalStagedDirectory::test_review_tree_sync_stops_after_nested_directory_failure();
}
