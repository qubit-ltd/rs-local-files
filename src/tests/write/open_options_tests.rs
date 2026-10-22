// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for native `OpenOptions`.

use std::time::Duration;

use crate::write::Mode;
use crate::write::OpenOptions;

#[test]
fn test_open_options_builders_update_open_behavior() {
    let options = OpenOptions::new(Mode::AppendOrCreate)
        .with_parents()
        .with_open_retry_timeout(Duration::from_millis(5));
    assert_eq!(
        options.mode(),
        Mode::AppendOrCreate,
        "the configured write mode must be preserved"
    );
    assert!(options.creates_parents(), "with_parents must enable parent creation");
    assert_eq!(
        options.open_retry_timeout(),
        Some(Duration::from_millis(5)),
        "the configured open retry timeout must be preserved"
    );
    assert_eq!(
        OpenOptions::default().mode(),
        Mode::default(),
        "default options must use the default write mode"
    );
}
