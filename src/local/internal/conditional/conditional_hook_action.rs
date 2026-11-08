// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

/// One deterministic action consumed by a backend checkpoint.
///
/// Checkpoints let the shared commit protocol validate state transitions at
/// native-operation boundaries. Test hooks may inject a failure or competing
/// namespace change at those boundaries; production backends otherwise treat
/// them as no-ops.
#[derive(Clone, Debug)]
#[allow(dead_code)] // The fake backend constructs these actions in deterministic tests.
pub(crate) enum ConditionalHookAction {
    /// Leaves the checkpoint outcome unchanged.
    Noop,
    /// Returns an injected failure with the supplied kind.
    Fail(
        /// Error kind returned at the checkpoint.
        io::ErrorKind,
    ),
    /// Replaces the destination contents with the supplied bytes.
    ReplaceTarget(
        /// Replacement destination contents.
        Vec<u8>,
    ),
    /// Changes the destination contents in place to the supplied bytes.
    OverwriteTargetInPlace(
        /// Replacement bytes written without replacing the entry.
        Vec<u8>,
    ),
    /// Replaces candidate-copy contents with invalid bytes.
    CorruptCandidateCopy(
        /// Invalid bytes written to the candidate copy.
        Vec<u8>,
    ),
    /// Leaves the candidate copy containing only the supplied partial bytes.
    PartialCandidateCopy(
        /// Bytes left in the incomplete candidate copy.
        Vec<u8>,
    ),
    /// Simulates publication that changed only part of the namespace.
    PartialPublish,
}
