// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Publication policy enforced by the final atomic installation step.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum LocalAtomicPublicationMode {
    /// Install only when the destination is still absent.
    CreateNew,
    /// Create an absent destination or replace the entry observed at open.
    #[default]
    ReplaceOrCreate,
}
