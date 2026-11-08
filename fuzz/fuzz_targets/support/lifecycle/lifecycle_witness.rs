// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Lifecycle outcome counters used by deterministic fuzz fixtures.

/// Counts lifecycle outcomes for deterministic, input-selected fixtures.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct LifecycleWitness {
    /// Number of temporary resources created by the fixture.
    pub(super) created: usize,
    /// Number of resources successfully published.
    pub(super) successful_publications: usize,
    /// Number of explicit cleanup calls that succeeded.
    pub(super) explicit_cleanups: usize,
    /// Number of resources observed in the released state.
    pub(super) released: usize,
    /// Number of released resources whose artifacts were absent.
    pub(super) artifacts_absent: usize,
}
