// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for native path-length limits.

use qubit_local_files::LocalFileSystemLimits;
use qubit_local_files::SizeLimit;

/// Verifies a path-length limit preserves both its numeric bound and unit.
#[test]
fn test_local_file_system_limits_preserve_each_dimension() {
    let limits = LocalFileSystemLimits::new(SizeLimit::Maximum(260), SizeLimit::Unknown);

    assert_eq!(SizeLimit::Maximum(260), limits.max_path_bytes());
    assert_eq!(SizeLimit::Unknown, limits.max_file_name_bytes());
}
