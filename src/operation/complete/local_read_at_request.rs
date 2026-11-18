// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::SeekFrom;
use std::path::PathBuf;

use crate::options::LocalReadOptions;

/// Owned input to a bounded read at a position in one opened file object.
///
/// The request is consumed when accepted and returned intact when submission
/// is rejected. Host-relative paths are joined to the facade's captured base
/// before enqueue; Rooted paths are opened under its retained authority by the
/// worker. The read options are the complete reader configuration for this
/// request; filesystem default read options are not merged into them. They do
/// not replace the executor's resource limits.
#[derive(Debug)]
pub struct LocalReadAtRequest {
    /// Path opened under the filesystem's captured authority.
    pub path: PathBuf,
    /// Options for opening the reader, including its symlink and retry policy.
    pub read: LocalReadOptions,
    /// Position applied with [`Seek`](std::io::Seek::seek) to the newly opened
    /// reader before bytes are read. `Current` and `End` are relative to that
    /// reader's initial position and file length, respectively.
    pub offset: SeekFrom,
    /// Maximum number of bytes returned. The read may return fewer bytes at
    /// EOF; zero returns an empty buffer after the requested seek succeeds.
    pub max_bytes: usize,
}
