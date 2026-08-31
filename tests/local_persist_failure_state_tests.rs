// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public persistence failure-state coverage.

use qubit_local_files::outcome::LocalPersistFailureState;

/// Verifies the exposed persistence failure states are distinct.
#[test]
fn test_local_persist_failure_states_are_distinct() {
    assert_ne!(
        LocalPersistFailureState::NotPublished,
        LocalPersistFailureState::Indeterminate
    );
}
