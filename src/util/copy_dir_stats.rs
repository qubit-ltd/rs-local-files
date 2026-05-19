/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
/// Statistics reported by recursive directory copy operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CopyDirStats {
    /// Number of regular files copied.
    pub files: u64,

    /// Number of destination directories created.
    pub directories: u64,

    /// Number of bytes copied from regular files.
    pub bytes: u64,
}
