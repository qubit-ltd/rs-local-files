// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(all(feature = "digest", feature = "test-support"))]
#[allow(dead_code)]
pub(crate) mod conditional;
#[allow(dead_code)]
pub(crate) mod conditional_temp_directory;
#[cfg(windows)]
#[allow(dead_code)]
pub(crate) mod windows_symlink;
