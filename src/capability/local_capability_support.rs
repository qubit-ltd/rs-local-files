// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Reports whether the platform can attempt a requested native protocol.
///
/// `CanAttempt` only describes compiled support; the operation may still fail
/// at runtime because of the filesystem or operating-system environment.
///
/// # Examples
///
/// ```
/// use qubit_local_files::capability::LocalCapabilitySupport;
///
/// let support = LocalCapabilitySupport::CanAttempt;
/// assert!(matches!(support, LocalCapabilitySupport::CanAttempt));
/// ```
#[must_use = "capability support should be checked before attempting the protocol"]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalCapabilitySupport {
    /// The platform does not implement the requested protocol.
    Unsupported,
    /// The implementation can attempt the protocol at runtime.
    CanAttempt,
}
