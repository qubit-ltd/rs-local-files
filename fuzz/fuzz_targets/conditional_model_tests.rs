// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Fixed regression sequences for the conditional-write state machine.

#[path = "support/conditional_model.rs"]
mod conditional_model;
mod support;

#[test]
fn fixed_conditional_sequences_hit_their_oracles() {
    for rooted in [false, true] {
        for (name, data) in conditional_model::regression_sequences() {
            let witness = conditional_model::run_model(data, rooted)
                .unwrap_or_else(|| panic!("{name}: scratch root setup failed"));
            conditional_model::assert_regression(name, &witness, rooted);
        }
    }
}
