// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceCapacityError;
use qubit_operation::ResourceId;

/// Creation failure, including the resource identity when cleanup was retained.
#[derive(Debug)]
pub enum LocalCreateFailure {
    /// Scope capacity was unavailable before native creation.
    Reservation(
        /// Capacity reservation failure returned before native creation.
        ResourceCapacityError,
    ),
    /// Existing synchronous temp creation failed.
    Local(
        /// Native filesystem creation error.
        crate::LocalFileError,
    ),
    /// Native creation was partial and its cleanup owner was retained.
    PartialRetained {
        /// Original filesystem failure that prevented full initialization.
        /// Original filesystem failure that prevented full initialization.
        error: crate::LocalFileError,
        /// Resource id retaining the partially created native owner.
        /// Stable identity of the resource retained for cleanup.
        resource_id: ResourceId,
    },
    /// Commit was rejected and the original resource was retained for cleanup.
    Retained {
        /// Resource manager's reason for retaining the owner.
        /// Resource manager reason for retaining the native owner.
        reason: qubit_operation::ResourceManageErrorKind,
        /// Stable resource id that remains registered for cleanup.
        /// Stable identity of the resource retained for cleanup.
        resource_id: ResourceId,
    },
    /// Executor capacity was unavailable before native creation.
    Capacity,
}
