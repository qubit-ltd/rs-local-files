// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fuzzes a bounded conditional-write sequence.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "support/conditional_model.rs"]
mod conditional_model;

mod support;

fuzz_target!(|data: &[u8]| {
    for rooted in [false, true] {
        conditional_model::run_model(data, rooted)
            .unwrap_or_else(|error| panic!("conditional fuzz harness setup failure: {error}"));
    }
});
