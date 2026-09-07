// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thread-local primitive observations used only by library test builds.

use std::cell::Cell;

thread_local! {
    static METADATA: Cell<usize> = const { Cell::new(0) };
    static DIRECTORY_OPEN: Cell<usize> = const { Cell::new(0) };
    static FALLBACK: Cell<usize> = const { Cell::new(0) };
}

/// Records a metadata attempt at the cursor's native operation boundary.
pub(crate) fn record_metadata() {
    METADATA.set(METADATA.get() + 1);
}
/// Records a directory-open attempt at the cursor's native operation boundary.
pub(crate) fn record_directory_open() {
    DIRECTORY_OPEN.set(DIRECTORY_OPEN.get() + 1);
}
/// Records entry to the general resolver, which must not serve normal paths.
pub(crate) fn record_fallback() {
    FALLBACK.set(FALLBACK.get() + 1);
}
/// Clears only this test thread's observations before a complete resolver call.
pub(crate) fn reset() {
    METADATA.set(0);
    DIRECTORY_OPEN.set(0);
    FALLBACK.set(0);
}
/// Returns metadata, directory-open, and fallback observations in that order.
pub(crate) fn snapshot() -> (usize, usize, usize) {
    (METADATA.get(), DIRECTORY_OPEN.get(), FALLBACK.get())
}
