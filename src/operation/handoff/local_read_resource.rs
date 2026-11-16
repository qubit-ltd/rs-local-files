// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceRef;

use super::super::temp::LocalTempResource;

/// Managed resource identity shared by stage, helper, and export wrappers.
///
/// This indirection lets every exported handle retain the same registered
/// resource without adding another resource-manager entry.
pub struct LocalReadResource {
    /// Registered owner whose native sealed file is accessed by the lease.
    pub(super) reference: ResourceRef<LocalTempResource>,
    /// Whether the caller explicitly permitted exposing a Rooted handle.
    pub(super) export_read_handle: bool,
}
