// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for the `LocalFiles` namespace, split by filesystem responsibility.

use qubit_local_files::LocalFiles;

mod atomic_write_tests;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod copy_dir_tests;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod copy_dir_unsupported_tests;
mod file_io_tests;
mod path_operations_tests;

#[test]
fn test_default_temp_entry_retries_is_positive() {
    const { assert!(LocalFiles::DEFAULT_TEMP_ENTRY_RETRIES > 0) };
}
