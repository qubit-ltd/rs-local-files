// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Value coverage for local copy source modes.

use qubit_local_files::options::LocalCopySourceMode;

/// Verifies automatic source detection remains the conservative default.
#[test]
fn test_local_copy_source_mode_defaults_to_auto() {
    assert_eq!(LocalCopySourceMode::Auto, LocalCopySourceMode::default());
}

/// Verifies all public source interpretations remain distinct.
#[test]
fn test_local_copy_source_modes_are_distinct() {
    assert_ne!(LocalCopySourceMode::File, LocalCopySourceMode::Tree);
    assert_ne!(LocalCopySourceMode::Tree, LocalCopySourceMode::Auto);
    assert_ne!(LocalCopySourceMode::Auto, LocalCopySourceMode::File);
}
