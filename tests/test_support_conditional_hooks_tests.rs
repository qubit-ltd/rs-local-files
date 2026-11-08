// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(all(feature = "digest", feature = "test-support"))]

use std::io;

use qubit_local_files::test_support::TestConditionalHooks;
use qubit_local_files::test_support::TestConditionalPoint;

#[test]
fn test_debug_shows_hook_point_without_exposing_action() {
    let hooks = TestConditionalHooks::once(TestConditionalPoint::BeforePublish, || Ok::<(), io::Error>(()));

    let debug = format!("{hooks:?}");
    assert!(debug.contains("BeforePublish"));
    assert!(!debug.contains("action"));
}
