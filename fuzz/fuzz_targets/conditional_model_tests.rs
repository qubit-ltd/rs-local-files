// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fixed regression sequences for the conditional-write state machine.

#[path = "support/conditional_model.rs"]
mod conditional_model;
mod support;

#[test]
fn test_fixed_conditional_sequences_hit_their_oracles() {
    for rooted in [false, true] {
        for (name, data) in conditional_model::regression_sequences() {
            let witness = conditional_model::run_model(data, rooted)
                .unwrap_or_else(|error| panic!("{name}, rooted={rooted}: {error}"));
            conditional_model::assert_regression(name, &witness, rooted);
        }
    }
}

#[test]
fn test_rooted_model_setup_reports_open_failure() {
    let root = support::FuzzRoot::create("conditional-setup-test").expect("create isolated test root");
    let missing = root.path().join("missing");
    let error = conditional_model::prepare_filesystem(&missing, true).expect_err("missing Rooted authority must fail");
    assert!(error.contains("Rooted"), "{error}");
    assert!(error.contains("filesystem open"), "{error}");
}
