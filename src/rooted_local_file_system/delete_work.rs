// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::local::LocalRelativePath;

/// Pending work for one no-follow Rooted recursive deletion.
pub(super) enum DeleteWork {
    /// Inspects an entry before deciding how to remove it.
    Inspect(LocalRelativePath),
    /// Removes a directory after all of its children have been processed.
    RemoveDirectory(LocalRelativePath),
}
