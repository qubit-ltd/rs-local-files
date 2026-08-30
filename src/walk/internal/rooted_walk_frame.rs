// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by rooted walker integration tests.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;

use qubit_budget::ManagedResourcePermit;

use crate::LocalResourceKind;
use crate::rooted::DirectoryReader;

/// One deferred immediate rooted directory listing in a lazy tree walk.
#[derive(Debug)]
pub(in crate::walk) struct RootedWalkFrame {
    /// Open immediate-entry reader, initialized on the first iteration step.
    pub(in crate::walk) reader: Option<DirectoryReader>,
    /// Capacity permit retained for exactly as long as `reader` is open.
    pub(in crate::walk) directory_permit: Option<ManagedResourcePermit<LocalResourceKind, usize>>,
    /// Names already yielded from this directory.
    pub(in crate::walk) seen: HashSet<OsString>,
    /// Authority-relative path of the listed directory.
    pub(in crate::walk) authority_parent: PathBuf,
    /// Requested-list-root-relative path of the listed directory.
    pub(in crate::walk) output_parent: PathBuf,
    /// Depth assigned to entries returned by this frame.
    pub(in crate::walk) entry_depth: usize,
    /// Native directory identity retained for cycle detection.
    pub(in crate::walk) identity: Option<crate::local::DirectoryIdentity>,
}
