/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use super::local_files_tests::LocalCopyDirOptions;

#[test]
fn test_copy_dir_options_default_is_conservative() {
    let options = LocalCopyDirOptions::default();

    assert!(!options.overwrite);
    assert!(!options.follow_symlinks);
    assert!(!options.preserve_permissions);
}
