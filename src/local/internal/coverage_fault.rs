// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic fault selection for coverage-only subprocess tests.

use std::ffi::OsStr;

/// Environment variable carrying one isolated coverage fault name.
const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";

/// Returns whether the isolated test process selected `name`.
///
/// # Parameters
///
/// * `name` - Static name of a fault at a real native operation boundary.
///
/// # Returns
///
/// `true` only when the coverage subprocess selected exactly `name`.
pub(super) fn is_enabled(name: &str) -> bool {
    std::env::var_os(COVERAGE_FAULT_ENV)
        .is_some_and(|value| value == OsStr::new(name))
}
