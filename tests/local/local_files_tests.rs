// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for the `LocalFiles` namespace, split by filesystem responsibility.

mod atomic_write_tests;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod copy_dir_tests;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod copy_dir_unsupported_tests;
mod file_io_tests;
mod path_operations_tests;
