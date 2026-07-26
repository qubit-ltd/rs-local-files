// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::api_tests::LocalPersistOptions;

#[test]
fn test_persist_options_default_is_conservative() {
    let options = LocalPersistOptions::default();

    assert!(!options.overwrites());
}

#[test]
fn test_persist_options_builder_enables_overwrite() {
    let options = LocalPersistOptions::new().with_overwrite();

    assert!(options.overwrites());
}
