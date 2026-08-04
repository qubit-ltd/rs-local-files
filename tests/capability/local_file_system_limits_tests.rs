// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Coverage for native filesystem limit values.

use qubit_local_files::{
    LocalFileSystemLimits,
    SizeLimit,
};

/// Verifies limits preserve independent finite and unrestricted dimensions.
#[test]
fn test_local_file_system_limits_preserve_independent_values() {
    let limits = LocalFileSystemLimits::new(
        SizeLimit::Maximum(4096),
        SizeLimit::Unrestricted,
    );

    assert_eq!(SizeLimit::Maximum(4096), limits.max_path_bytes());
    assert_eq!(SizeLimit::Unrestricted, limits.max_file_name_bytes());
}
