// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Failure to schedule or execute a managed native-resource command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalTempCommandError {
    /// The executor could not accept another task.
    Capacity,
    /// The manager no longer contains a native owner for this resource.
    ResourceUnavailable,
    /// This resource kind does not support the requested transition.
    UnsupportedKind,
    /// A crate-controlled native command unwound with a panic.
    Panicked,
}
