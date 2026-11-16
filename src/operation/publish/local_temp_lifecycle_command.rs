// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

/// One owned lifecycle action for a previously managed temporary resource.
#[derive(Clone, Debug)]
pub enum LocalTempLifecycleCommand {
    /// Atomically keep the temporary resource at a generated destination.
    Keep,
    /// Atomically persist at the requested path.
    Persist {
        /// Target interpreted using the temporary resource's captured
        /// authority.
        target: PathBuf,
        /// Persistence guarantees for this transition.
        options: crate::options::LocalPersistOptions,
    },
    /// Seal a temporary file as read-only.
    Seal {
        /// File synchronization policy for sealing.
        options: crate::LocalSealOptions,
    },
}
