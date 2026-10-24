// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Independent copy destination-effect merge tests.

use crate::outcome::LocalCopyFailureState;
use crate::outcome::merge_copy_failure_state;

#[test]
fn test_copy_failure_state_merges_independent_destination_effects() {
    let cases = [
        (LocalCopyFailureState::Unchanged, None, LocalCopyFailureState::Unchanged),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::Published,
        ),
    ];

    for (inferred, hint, expected) in cases {
        assert_eq!(expected, merge_copy_failure_state(inferred, hint));
    }
}
