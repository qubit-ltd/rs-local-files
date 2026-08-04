// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for capability-support states.

use qubit_local_files::LocalFileSystemCapabilitySupport;

/// Verifies capability-support states remain distinct.
#[test]
fn test_local_file_system_capability_support_states_are_distinct() {
    assert_ne!(
        LocalFileSystemCapabilitySupport::Implemented,
        LocalFileSystemCapabilitySupport::RuntimeVerified,
    );
    assert_ne!(
        LocalFileSystemCapabilitySupport::RuntimeVerified,
        LocalFileSystemCapabilitySupport::Unknown,
    );
}
