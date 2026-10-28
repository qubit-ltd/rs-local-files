// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Presence or support status of an ACL mechanism.
///
/// `Unknown` means the observation could not establish presence or absence; it
/// does not mean that access is unrestricted.
pub enum LocalAclPresence {
    /// The ACL is known to be absent.
    Absent,
    /// An ACL is present.
    Present,
    /// The platform or filesystem explicitly reports no support for the queried
    /// ACL kind.
    NotSupported,
}
