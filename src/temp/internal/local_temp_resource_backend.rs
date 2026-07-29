// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- paired internal state types form one
// temporary-resource boundary.
//! Shared authority marker for unified temporary resources.

use super::{
    HostTempResourceBackend,
    RootedTempResourceBackend,
};

/// Identifies whether a temporary resource is host- or descriptor-relative.
#[derive(Debug)]
pub(crate) enum LocalTempResourceBackend {
    /// A host path already bound at creation time.
    Host(HostTempResourceBackend),
    /// A descendant authorized by an opened root handle.
    Rooted(RootedTempResourceBackend),
}

/// Namespace certainty retained after a temporary-resource state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalTempResourceState {
    /// The source is known to remain owned and cleanup-safe.
    Owned,
    /// The native namespace result is unknown; no cleanup operation is safe.
    Indeterminate,
    /// The resource was kept, cleaned, or fully persisted.
    Released,
}
