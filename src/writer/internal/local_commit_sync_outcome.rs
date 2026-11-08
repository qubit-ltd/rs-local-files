// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Reports file synchronization and the resulting commit durability state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalCommitSyncOutcome {
    /// Whether the commit meets its configured durability requirement.
    pub(crate) durable: bool,
    /// Whether the committed file itself was synchronized.
    pub(crate) file_synced: bool,
}
