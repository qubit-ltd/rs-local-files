// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Whether a platform can attempt a requested native protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalCapabilitySupport {
    /// The platform does not implement the requested protocol.
    Unsupported,
    /// The implementation can attempt the protocol at runtime.
    CanAttempt,
}
