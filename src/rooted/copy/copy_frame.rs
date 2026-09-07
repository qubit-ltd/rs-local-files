// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resources retained for one active Rooted copy directory.

use qubit_budget::ManagedResourcePermit;

use crate::LocalResourceKind;
use crate::rooted::DirectoryReader;
use crate::rooted::Metadata;
use crate::rooted::Path;

/// One active rooted directory reader retained until its children finish.
#[derive(Debug)]
pub(super) struct CopyFrame {
    /// Validated authority-relative source directory.
    pub(super) source: Path,
    /// Validated authority-relative destination directory.
    pub(super) destination: Path,
    /// Source directory metadata retained for post-order preservation.
    pub(super) metadata: Metadata,
    /// Logical descendant depth used by the shared scheduler.
    pub(super) depth: usize,
    /// Owned lazy native directory reader.
    pub(super) reader: DirectoryReader,
    /// Budget capacity declared after the reader so implicit drop closes the
    /// native resource before returning its capacity.
    pub(super) directory_permit: Option<ManagedResourcePermit<LocalResourceKind, usize>>,
}
