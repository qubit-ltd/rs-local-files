// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::rooted::EntryKind;

/// Verifies every portable rooted entry kind remains distinguishable.
#[test]
fn test_entry_kind_variants_are_distinct() {
    assert_ne!(EntryKind::File, EntryKind::Directory);
    assert_ne!(EntryKind::Directory, EntryKind::Symlink);
    assert_ne!(EntryKind::Symlink, EntryKind::Other);
}
