// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;

use super::LocalReadLease;

/// Read-only file handle and the lease that must accompany its use.
/// Any additional `File::try_clone` used by a helper must be paired with an
/// additional cloned lease for the entire duration of that helper's use.
pub struct LocalReadExport {
    /// Read-only handle cloned from the managed sealed file.
    pub file: File,
    /// Keeps the managed owner and cleanup responsibility alive.
    pub lease: LocalReadLease,
}
