// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native method used to publish writer output.

/// Native method used by a completed writer session.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalWritePublicationMethod {
    /// Bytes were staged and installed with an atomic rename.
    AtomicRename,
    /// Bytes were written directly to an existing destination.
    DirectAppend,
}
