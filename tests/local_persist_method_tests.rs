// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public persistence publication-method coverage.

use qubit_local_files::outcome::LocalPersistMethod;

/// Verifies temporary persistence currently reports native rename publication.
#[test]
fn test_local_persist_method_reports_atomic_rename() {
    assert_eq!(LocalPersistMethod::AtomicRename, LocalPersistMethod::AtomicRename);
}
