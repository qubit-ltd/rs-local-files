// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared authority marker for unified temporary resources.
// qubit-style: allow source-test-pair
// Covered through the public temporary-resource integration tests.

use super::HostTempResourceBackend;
use super::RootedTempResourceBackend;

/// Identifies whether a temporary resource is host- or descriptor-relative.
#[derive(Debug)]
pub(crate) enum LocalTempResourceBackend {
    /// A host path already bound at creation time.
    Host(
        /// Bound Host resource ownership state.
        HostTempResourceBackend,
    ),
    /// A descendant authorized by an opened root handle.
    Rooted(
        /// Handle-authoritative Rooted resource ownership state.
        RootedTempResourceBackend,
    ),
}
