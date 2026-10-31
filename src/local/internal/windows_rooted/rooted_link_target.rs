// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// Bounded raw substitute target and reparse flags, before namespace
/// normalization.
#[derive(Debug)]
pub(crate) struct RootedLinkTarget {
    /// Raw substitute spelling; policy checks run before namespace
    /// normalization.
    pub(crate) raw_path: PathBuf,
    /// Native reparse flags that determine whether the substitute name is
    /// relative.
    pub(crate) flags: u32,
}
// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
