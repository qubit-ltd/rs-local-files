// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalDurabilityState;
use super::LocalPublicationState;
use super::LocalTempLifecycleError;
use super::LocalTempLifecycleResult;
use super::LocalTempNative;

/// Result of one serialized lifecycle transition and its retained owner state.
pub(super) type LocalTempNativeTransition = Result<
    (
        Option<LocalTempNative>,
        (LocalTempLifecycleResult, LocalPublicationState, LocalDurabilityState),
    ),
    (LocalTempNative, LocalTempLifecycleError),
>;
