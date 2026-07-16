// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::{
    LocalCopyConflictPolicy,
    LocalCopyDirOptions,
    LocalCopyTypeConflictPolicy,
};

#[test]
fn test_copy_dir_options_default_is_conservative() {
    let options = LocalCopyDirOptions::default();

    assert_eq!(LocalCopyConflictPolicy::Fail, options.conflict_policy());
    assert_eq!(
        LocalCopyTypeConflictPolicy::Fail,
        options.type_conflict_policy()
    );
    assert!(!options.follows_symlinks());
    assert!(!options.preserves_permissions());
}

#[test]
fn test_copy_dir_options_builders_express_non_default_policies() {
    let options = LocalCopyDirOptions::new()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .follow_symlinks()
        .preserve_permissions();

    assert_eq!(
        LocalCopyConflictPolicy::Overwrite,
        options.conflict_policy()
    );
    assert_eq!(
        LocalCopyTypeConflictPolicy::Replace,
        options.type_conflict_policy()
    );
    assert!(options.follows_symlinks());
    assert!(options.preserves_permissions());
}
