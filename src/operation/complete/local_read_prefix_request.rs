// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalReadOptions;

/// Owned input to a read of at most a file's initial prefix.
///
/// The request is consumed when accepted and returned intact when submission
/// is rejected. Host-relative paths are joined to the facade's captured base
/// before enqueue; Rooted paths are opened under its retained authority by the
/// worker. The read options are the complete reader configuration for this
/// request; filesystem default read options are not merged into them. They do
/// not replace the executor's resource limits. One extra byte may be read to
/// determine whether the requested prefix ended at EOF; if the shared owned
/// byte pool cannot retain the requested prefix, the operation reports a
/// resource-limit error instead of returning a truncated success. The extra
/// byte used to probe for EOF is a stack buffer and is not charged to the pool.
#[derive(Debug)]
pub struct LocalReadPrefixRequest {
    /// Path opened under the filesystem's captured authority.
    pub path: PathBuf,
    /// Options for opening the reader, including its symlink and retry policy.
    pub read: LocalReadOptions,
    /// Maximum number of prefix bytes retained in the result. If zero, no
    /// bytes are returned; `complete` reports whether the opened file was
    /// empty.
    pub max_bytes: usize,
}
