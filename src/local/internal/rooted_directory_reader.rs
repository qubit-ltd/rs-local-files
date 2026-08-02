// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Platform-specific state for lazy Rooted directory enumeration.
// qubit-style: allow source-test-pair

use std::fs::File;

#[cfg(unix)]
use rustix::fs::Dir;

/// Lazily reads children from one already-opened Rooted directory.
#[derive(Debug)]
pub(crate) struct RootedDirectoryReader {
    /// Descriptor or handle retained for enumeration and child inspection.
    pub(super) directory: File,
    /// Native Unix directory stream.
    #[cfg(unix)]
    pub(super) stream: Dir,
    /// Path used only to enrich Unix native I/O errors.
    #[cfg(unix)]
    pub(super) diagnostic_path: std::path::PathBuf,
    /// Aligned storage for Windows native directory records.
    #[cfg(windows)]
    pub(super) buffer: Vec<usize>,
    /// Number of valid bytes currently in the Windows buffer.
    #[cfg(windows)]
    pub(super) used: usize,
    /// Offset of the next Windows native record within the buffer.
    #[cfg(windows)]
    pub(super) offset: usize,
    /// Whether the next Windows request must restart enumeration.
    #[cfg(windows)]
    pub(super) restart: bool,
    /// Whether Windows reported that enumeration is complete.
    #[cfg(windows)]
    pub(super) exhausted: bool,
}
