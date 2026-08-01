// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for native path-length limits.

use qubit_local_files::{
    LocalPathLengthUnit,
    LocalPathLimit,
};

/// Verifies a path-length limit preserves both its numeric bound and unit.
#[test]
fn test_local_path_limit_preserves_value_and_unit() {
    let limit = LocalPathLimit::new(260, LocalPathLengthUnit::Utf16CodeUnits);

    assert_eq!(limit.value(), 260);
    assert_eq!(limit.unit(), LocalPathLengthUnit::Utf16CodeUnits);
}
