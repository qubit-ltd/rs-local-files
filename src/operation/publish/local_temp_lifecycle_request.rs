// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::super::temp::LocalTempFileAccess;

/// Owned temporary-resource lifecycle request.
pub struct LocalTempLifecycleRequest {
    /// Existing access token for the registered temporary resource.
    pub access: super::LocalTempFileAccess,
    /// Native lifecycle action to serialize on the local worker.
    pub command: LocalTempLifecycleCommand,
}
