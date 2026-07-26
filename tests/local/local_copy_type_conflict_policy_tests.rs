// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::api_tests::LocalCopyTypeConflictPolicy;

#[test]
fn test_copy_type_conflict_policy_default_rejects_replacement() {
    assert_eq!(
        LocalCopyTypeConflictPolicy::Fail,
        LocalCopyTypeConflictPolicy::default()
    );
}
