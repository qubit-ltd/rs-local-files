// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Pending work for one no-follow Host recursive deletion.
pub(super) enum DeleteWork {
    /// Inspects an entry before deciding how to remove it.
    Inspect(PathBuf),
    /// Removes a directory after all of its children have been processed.
    RemoveDirectory(PathBuf),
}
