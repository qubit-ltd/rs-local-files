// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceRef;

use super::LocalWalkResource;
use super::map_walk_error;

/// Observer for background walker stop and native-handle close confirmation.
///
/// Obtain an observer before dropping a walker when the caller needs to wait
/// for worker-confirmed native closure.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalWalkCloseObserver;
/// use qubit_local_files::LocalResult;
///
/// async fn wait(observer: &LocalWalkCloseObserver) -> LocalResult<()> {
///     observer.wait_closed().await
/// }
/// ```
pub struct LocalWalkCloseObserver {
    /// Worker resource while close is pending; absent after confirmation.
    pub(super) resource: Option<ResourceRef<LocalWalkResource>>,
}

impl LocalWalkCloseObserver {
    /// Creates an observer while worker close is still pending.
    ///
    /// # Parameters
    /// - `resource` — worker resource whose close confirmation is pending.
    pub(super) fn observing(resource: ResourceRef<LocalWalkResource>) -> Self {
        Self {
            resource: Some(resource),
        }
    }

    /// Creates an observer for an already confirmed close.
    pub(super) fn closed() -> Self {
        Self { resource: None }
    }

    /// Waits for the serial worker's native handle close acknowledgement.
    pub async fn wait_closed(&self) -> crate::LocalResult<()> {
        match &self.resource {
            Some(resource) => {
                let mut registration = resource.close_registration();
                std::future::poll_fn(|cx| resource.poll_closed(cx, registration.as_mut()))
                    .await
                    .map_err(map_walk_error)
            }
            None => Ok(()),
        }
    }
}
