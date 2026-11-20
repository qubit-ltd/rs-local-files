// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::task::Context;
use std::task::Poll;

use super::LocalStreamCompletion;
use super::LocalStreamRequest;

/// Private route for writing from an existing shared stream payload.
pub(super) trait NativeStreamWriter: Send + Sync {
    /// Polls one write attempt starting at `offset` in the original payload.
    ///
    /// # Parameters
    ///
    /// * `cx` - Poll context used to register the task waker.
    /// * `request` - Original write request and shared payload.
    /// * `offset` - Number of payload bytes already confirmed written.
    fn poll_native_write(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
        offset: usize,
    ) -> Poll<io::Result<LocalStreamCompletion>>;
}
