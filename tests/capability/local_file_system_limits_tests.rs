// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for native filesystem limit values.

use qubit_local_files::LocalFileSystemLimits;
use qubit_local_files::SizeLimit;

/// Verifies limits preserve independent finite and path-dependent dimensions.
#[test]
fn test_local_file_system_limits_preserve_independent_values() {
    let limits = LocalFileSystemLimits::new(SizeLimit::Maximum(4096), SizeLimit::VariesByPath);

    assert_eq!(SizeLimit::Maximum(4096), limits.max_path_bytes());
    assert_eq!(SizeLimit::VariesByPath, limits.max_file_name_bytes());
}
