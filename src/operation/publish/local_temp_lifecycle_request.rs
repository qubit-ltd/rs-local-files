// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::super::temp::LocalTempFileAccess;
use super::LocalTempLifecycleCommand;

/// Owned temporary-resource lifecycle request.
pub struct LocalTempLifecycleRequest {
    /// Existing access token for the registered temporary resource.
    /// Access token that identifies and retains the managed resource.
    pub access: LocalTempFileAccess,
    /// Native lifecycle action to serialize on the local worker.
    /// Transition serialized by the local worker.
    pub command: LocalTempLifecycleCommand,
}
