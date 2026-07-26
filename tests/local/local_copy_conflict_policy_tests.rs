// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::api_tests::LocalCopyConflictPolicy;

#[test]
fn test_copy_conflict_policy_default_fails_on_conflicts() {
    assert_eq!(
        LocalCopyConflictPolicy::Fail,
        LocalCopyConflictPolicy::default()
    );
}
