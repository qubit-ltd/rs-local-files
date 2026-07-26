// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::api_tests::LocalAtomicDestinationState;

#[test]
fn test_atomic_destination_state_variants_are_distinct_and_debuggable() {
    let states = [
        LocalAtomicDestinationState::Unchanged,
        LocalAtomicDestinationState::Replaced,
        LocalAtomicDestinationState::Missing,
        LocalAtomicDestinationState::Indeterminate,
    ];

    for (index, state) in states.iter().enumerate() {
        assert_eq!(*state, states[index]);
        assert!(!format!("{state:?}").is_empty());
    }
}
