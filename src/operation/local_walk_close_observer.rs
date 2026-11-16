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
    /// Worker resource whose close acknowledgement this observer polls.
    pub(super) resource: ResourceRef<LocalWalkResource>,
}

impl LocalWalkCloseObserver {
    /// Waits for the serial worker's native handle close acknowledgement.
    pub async fn wait_closed(&self) -> crate::LocalResult<()> {
        std::future::poll_fn(|cx| self.resource.poll_closed(cx))
            .await
            .map_err(map_walk_error)
    }
}
