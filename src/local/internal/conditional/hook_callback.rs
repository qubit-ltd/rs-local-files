// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Callback type used by test-support conditional hooks.

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

/// Shares a one-shot test callback between cloned conditional hook sets.
///
/// The callback is removed from its mutex before it is invoked, so user code
/// runs without holding the callback lock.
pub(super) type HookCallback = Arc<Mutex<Option<Box<dyn FnOnce() -> io::Result<()> + Send>>>>;
