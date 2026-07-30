// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native method used to publish a temporary resource.

/// Native method used to publish a temporary resource.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPersistMethod {
    /// A same-authority native rename published the resource.
    AtomicRename,
}
