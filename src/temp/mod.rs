// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unified temporary resources backed by host or rooted authority.

mod local_temp_directory;
mod local_temp_file;

/// Private storage implementations for temporary resources.
pub(crate) mod internal;

pub use local_temp_directory::LocalTempDirectory;
pub use local_temp_file::LocalTempFile;
