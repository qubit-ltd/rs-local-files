// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared local filesystem instance state.

mod local_file_system_core;
mod local_file_system_defaults;

#[cfg(all(test, feature = "test-support"))]
mod local_file_system_core_tests;

pub(crate) use local_file_system_core::LocalFileSystemCore;
pub(crate) use local_file_system_defaults::LocalFileSystemDefaults;
