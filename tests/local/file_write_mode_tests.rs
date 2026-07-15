// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::FileWriteMode;

#[test]
fn test_file_write_mode_default_creates_or_truncates() {
    assert_eq!(FileWriteMode::CreateOrTruncate, FileWriteMode::default());
}
