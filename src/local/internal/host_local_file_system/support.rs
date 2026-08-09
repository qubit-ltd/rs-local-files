// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host support operations.
// qubit-style: allow source-test-pair

/// Returns an injected native I/O failure selected by test-support tests.
///
/// # Parameters
///
/// - `fault`: Stable selector for one facade-native I/O boundary.
///
/// # Returns
///
/// `Some` deterministic I/O error only when the matching test fault is
/// enabled; `None` otherwise.
fn test_io_fault(fault: &str) -> Option<io::Error> {
    crate::local::test_io_error(fault)
}
