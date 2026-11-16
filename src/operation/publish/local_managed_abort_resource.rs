// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Mutex;

use qubit_operation::ResourceId;

use crate::LocalFileError;

/// Worker-owned native resource that can be aborted during cancellation.
///
/// # Type Parameters
///
/// * `W` - Native writer type whose abort routine runs on the I/O worker.
pub(in crate::operation) struct LocalManagedAbortResource<W: Send + 'static> {
    /// Writer owner retained until abort succeeds or cleanup reports failure.
    pub(in crate::operation) native: Mutex<Option<W>>,
    /// Resource identifier assigned when the operation scope adopts the owner.
    pub(in crate::operation) id: Mutex<Option<ResourceId>>,
    /// Reserved queue capacity used if normal worker admission is saturated.
    pub(in crate::operation) cleanup_permit: Mutex<Option<super::super::internal::permits::Permit>>,
    /// Native abort operation, selected for the concrete writer type.
    pub(in crate::operation) abort: fn(&mut W) -> Result<(), LocalFileError>,
}

impl<W: Send + 'static> LocalManagedAbortResource<W> {
    /// Creates an empty cleanup owner retaining its worker permit and abort action.
    ///
    /// # Parameters
    ///
    /// * `cleanup_permit` - Reserved capacity for eventual worker cleanup.
    /// * `abort` - Native abort operation for the owned writer type.
    pub(in crate::operation) fn new(
        cleanup_permit: super::super::internal::permits::Permit,
        abort: fn(&mut W) -> Result<(), LocalFileError>,
    ) -> Self {
        Self {
            native: Mutex::new(None),
            id: Mutex::new(None),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            abort,
        }
    }
}
