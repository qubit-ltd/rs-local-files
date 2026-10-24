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
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::Unchanged,
        ),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
        (LocalCopyFailureState::PartiallyPublished, None, LocalCopyFailureState::PartiallyPublished),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
        (LocalCopyFailureState::Published, None, LocalCopyFailureState::Published),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Published,
        ),
        (LocalCopyFailureState::Indeterminate, None, LocalCopyFailureState::Indeterminate),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
    ];

    for (inferred, hint, expected) in cases {
        assert_eq!(expected, merge_copy_failure_state(inferred, hint));
    }
}
