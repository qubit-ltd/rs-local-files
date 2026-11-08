// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for explicit native limit states.

use qubit_local_files::capability::SizeLimit;

/// Verifies every public size-limit state remains distinguishable.
#[test]
fn test_size_limit_states_are_distinct() {
    assert_ne!(
        SizeLimit::Maximum(1),
        SizeLimit::VariesByPath,
        "a finite maximum must differ from a path-dependent limit"
    );
    assert_ne!(
        SizeLimit::Maximum(1),
        SizeLimit::Unknown,
        "a finite maximum must differ from an unavailable limit"
    );
    assert_ne!(
        SizeLimit::VariesByPath,
        SizeLimit::Unknown,
        "a path-dependent limit must differ from an unavailable limit"
    );
}
