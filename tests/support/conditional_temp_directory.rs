// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

use tempfile::TempDir;

// Resolve environment-provided links before creating fixtures; links
// constructed inside each fixture remain visible to the Reject policy being
// tested.
pub(crate) fn tempdir() -> io::Result<TempDir> {
    let base = std::fs::canonicalize(std::env::temp_dir())?;
    tempfile::tempdir_in(base)
}
