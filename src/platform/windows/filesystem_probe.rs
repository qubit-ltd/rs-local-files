// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Best-effort Windows filesystem probes.

use std::fs::File;

use crate::LocalFileSystemLimits;
use crate::LocalFileSystemSpace;
use crate::SizeLimit;

/// Reports path limits that Windows does not expose through the open handle.
pub(super) const fn limits(_file: &File) -> LocalFileSystemLimits {
    LocalFileSystemLimits::new(SizeLimit::Unknown, SizeLimit::Unknown)
}

/// Reports capacity values unavailable through the current handle-only probe.
pub(super) const fn space(_file: &File) -> LocalFileSystemSpace {
    LocalFileSystemSpace::new(None, None, None)
}
