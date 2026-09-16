// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for copy destination policy.

use crate::local::CopyDestinationAction;
use crate::local::decide_copy_destination;
use crate::options::LocalCopyConflictPolicy;
use crate::options::LocalCopyTypeConflictPolicy;

#[test]
fn test_copy_destination_policy_selects_create_and_merge_actions() {
    assert_eq!(
        decide_copy_destination(
            true,
            None,
            LocalCopyConflictPolicy::Fail,
            LocalCopyTypeConflictPolicy::Fail,
        ),
        Some(CopyDestinationAction::Create)
    );
    assert_eq!(
        decide_copy_destination(
            true,
            Some(true),
            LocalCopyConflictPolicy::Fail,
            LocalCopyTypeConflictPolicy::Fail,
        ),
        Some(CopyDestinationAction::Merge)
    );
}

#[test]
fn test_copy_destination_policy_applies_type_conflict_policy() {
    for (policy, expected) in [
        (LocalCopyTypeConflictPolicy::Fail, None),
        (
            LocalCopyTypeConflictPolicy::Replace,
            Some(CopyDestinationAction::Replace),
        ),
        (LocalCopyTypeConflictPolicy::Skip, Some(CopyDestinationAction::Skip)),
    ] {
        assert_eq!(
            decide_copy_destination(true, Some(false), LocalCopyConflictPolicy::Fail, policy,),
            expected
        );
    }
}

#[test]
fn test_copy_destination_policy_applies_file_conflict_policy() {
    for (policy, expected) in [
        (LocalCopyConflictPolicy::Fail, None),
        (LocalCopyConflictPolicy::Overwrite, Some(CopyDestinationAction::Replace)),
        (LocalCopyConflictPolicy::Skip, Some(CopyDestinationAction::Skip)),
    ] {
        assert_eq!(
            decide_copy_destination(false, Some(false), policy, LocalCopyTypeConflictPolicy::Fail,),
            expected
        );
    }
}
