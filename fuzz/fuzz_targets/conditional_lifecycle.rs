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

#[cfg(feature = "conditional")]
#[path = "support/conditional_model.rs"]
mod conditional_model;

mod support;

#[cfg(feature = "conditional")]
fuzz_target!(|data: &[u8]| {
    conditional_model::run_model(data, false);
});

#[cfg(not(feature = "conditional"))]
fuzz_target!(|data: &[u8]| {
    support::lifecycle::run(data, false);
});
