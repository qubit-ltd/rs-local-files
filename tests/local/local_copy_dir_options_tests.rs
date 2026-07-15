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

    assert_eq!(LocalCopyConflictPolicy::Fail, options.conflict);
    assert_eq!(LocalCopyTypeConflictPolicy::Fail, options.type_conflict);
    assert!(!options.follow_symlinks);
    assert!(!options.preserve_permissions);
}
