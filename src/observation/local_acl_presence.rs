// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Presence or support status of an ACL mechanism.
///
/// `Absent` means the queried ACL kind was successfully checked and is not
/// present. `Present` means valid ACL data was read. `NotSupported` is returned
/// only when the platform or filesystem explicitly reports that the queried
/// ACL kind is unsupported. Permission failures, I/O errors, malformed ACL
/// data, and configured resource-limit violations are errors rather than
/// presence states. These variants describe the queried ACL kind only and do
/// not by themselves establish effective access.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalAclPresence {
    /// The ACL is known to be absent.
    Absent,
    /// An ACL is present.
    Present,
    /// The platform or filesystem explicitly reports no support for the queried
    /// ACL kind.
    NotSupported,
}
