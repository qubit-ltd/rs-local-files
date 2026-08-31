// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for directory-reopen policies.

use qubit_local_files::options::LocalDirectoryReopenPolicy;

/// Verifies the two directory-reopen policies remain distinct.
#[test]
fn test_local_directory_reopen_policy_states_are_distinct() {
    assert_ne!(LocalDirectoryReopenPolicy::Fail, LocalDirectoryReopenPolicy::Reopen,);
}
