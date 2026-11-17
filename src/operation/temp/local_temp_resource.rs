// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use qubit_operation::ResourceId;

use super::LocalTempNative;
/// A thread-safe control port owning a native temporary file and its permits.
///
/// The resource manager stores this owner under one stable id. Its native
/// object is serialized by `native`, while reservations remain attached until
/// cleanup releases the registration.
pub struct LocalTempResource {
    /// Current native owner; `None` means cleanup or a consuming transition
    /// took it.
    pub(crate) native: Mutex<Option<LocalTempNative>>,
    /// Disk reservations retained for the full native resource lifetime.
    pub(crate) _disk: Mutex<Vec<crate::operation::internal::permits::DiskPermit>>,
    /// Handle capacity retained while the OS resource remains registered.
    pub(crate) _handle: Mutex<Option<crate::operation::internal::permits::Permit>>,
    /// Pre-reserved capacity for eventual asynchronous cleanup.
    pub(crate) cleanup_permit: Mutex<Option<crate::operation::internal::permits::Permit>>,
    /// Shared stable resource id, populated when manager registration succeeds.
    pub(crate) id: Arc<Mutex<Option<ResourceId>>>,
}

impl LocalTempResource {
    /// Constructs the owner with all capacity reservations already acquired.
    ///
    /// # Parameters
    ///
    /// * `native` — native filesystem owner transferred into resource
    ///   management.
    /// * `disk` — reservation charged for the resource's initial file contents.
    /// * `handle` — reservation accounting for the open OS handle.
    /// * `cleanup_permit` — reserved queue capacity for asynchronous cleanup.
    pub(super) fn new(
        native: LocalTempNative,
        disk: crate::operation::internal::permits::DiskPermit,
        handle: crate::operation::internal::permits::Permit,
        cleanup_permit: crate::operation::internal::permits::Permit,
    ) -> Self {
        Self {
            native: Mutex::new(Some(native)),
            _disk: Mutex::new(vec![disk]),
            _handle: Mutex::new(Some(handle)),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            id: Arc::new(Mutex::new(None)),
        }
    }
}
