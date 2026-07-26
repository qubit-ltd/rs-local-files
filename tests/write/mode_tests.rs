// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::write::Mode;

/// Verifies the conservative default native write mode.
#[test]
fn test_mode_default_creates_or_truncates() {
    assert_eq!(Mode::CreateOrTruncate, Mode::default());
}

/// Verifies every native open behavior remains explicitly selectable.
#[test]
fn test_mode_exposes_all_native_open_behaviors() {
    let modes = [
        Mode::OpenExistingAtStart,
        Mode::CreateNew,
        Mode::CreateOrTruncate,
        Mode::AppendExisting,
        Mode::AppendOrCreate,
    ];

    assert_eq!(5, modes.len());
}
