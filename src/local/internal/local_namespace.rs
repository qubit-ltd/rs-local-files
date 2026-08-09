// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private namespace carrier for [`LocalFileSystem`](crate::LocalFileSystem).
// qubit-style: allow source-test-pair

use crate::rooted_local_file_system::RootedLocalFileSystem;

/// Closed native namespace implementation selected at construction.
#[derive(Clone, Debug)]
pub(crate) enum LocalNamespace {
    /// Process-visible Host namespace.
    Host,
    /// Descriptor- or handle-relative Rooted namespace.
    Rooted(RootedLocalFileSystem),
}
