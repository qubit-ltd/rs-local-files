// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native operation boundaries supported by deterministic test faults.

/// Native operation boundary at which a deterministic test fault is injected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestFaultPoint {
    /// Metadata observation.
    Metadata,
    /// Directory walk opening.
    WalkOpen,
    /// Copy source read.
    CopyRead,
    /// Copy destination write.
    CopyWrite,
    /// Publication flush.
    PublicationFlush,
    /// Publication file synchronization.
    PublicationSyncFile,
    /// Publication installation.
    PublicationInstall,
    /// Publication parent synchronization.
    PublicationSyncParent,
    /// Publication cleanup.
    PublicationCleanup,
    /// Temporary-resource identity verification.
    TempIdentity,
    /// Temporary-resource cleanup.
    TempCleanup,
}
