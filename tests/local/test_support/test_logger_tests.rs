// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Silent logger installation for cleanup-error tests.

use std::sync::Once;

/// One-time initialization state for the shared test logger.
static LOGGER_INIT: Once = Once::new();

/// Logger that accepts records without emitting test output.
struct TestLogger;

impl log::Log for TestLogger {
    /// Accepts every record enabled by the global maximum level.
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    /// Discards an accepted test log record.
    fn log(&self, _record: &log::Record<'_>) {}

    /// Performs no work because this logger buffers no output.
    fn flush(&self) {}
}

/// Shared logger installed by cleanup-error tests.
static TEST_LOGGER: TestLogger = TestLogger;

/// Installs the shared logger at warning level when no logger is installed.
pub(crate) fn ensure_test_logger() {
    LOGGER_INIT.call_once(|| {
        if log::set_logger(&TEST_LOGGER).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    });
}
