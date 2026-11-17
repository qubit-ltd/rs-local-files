// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalTempLifecycleError;
use super::LocalTempLifecycleResult;

/// Submits a temporary lifecycle transition and returns the same resource
/// access token.
pub type LocalTempLifecycleOperation =
    qubit_operation::AsyncOperationHandle<LocalTempLifecycleResult, LocalTempLifecycleError>;
