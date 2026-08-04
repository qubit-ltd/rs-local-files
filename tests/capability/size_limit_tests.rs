// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Coverage for explicit native limit states.

use qubit_local_files::SizeLimit;

/// Verifies every public size-limit state remains distinguishable.
#[test]
fn test_size_limit_states_are_distinct() {
    assert_ne!(SizeLimit::Maximum(1), SizeLimit::Unrestricted);
    assert_ne!(SizeLimit::Unrestricted, SizeLimit::Unknown);
}
