// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Temporary-resource behavior is covered through public integration tests.
//! Host-bound temporary-resource storage.

use std::path::PathBuf;

use super::TempEntryIdentity;

/// Retains the private cleanup sandbox created beside a host resource.
#[derive(Debug)]
pub(crate) struct HostTempResourceBackend {
    /// Unique native identity captured at successful construction.
    pub(crate) identity: TempEntryIdentity,
    /// Directory that is removed after the resource is released.
    pub(crate) sandbox_path: PathBuf,
}
