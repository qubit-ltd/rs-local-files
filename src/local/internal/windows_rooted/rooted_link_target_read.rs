// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Result of reading a symbolic-link target under a byte limit.
#[derive(Debug)]
pub(crate) enum RootedLinkTargetRead {
    /// The bounded raw target and reparse flags, before namespace
    /// normalization.
    Target(super::rooted_link_target::RootedLinkTarget),
    /// The raw target exceeds the limit; this is its observed UTF-16 byte
    /// length.
    Exceeded(usize),
}
// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
