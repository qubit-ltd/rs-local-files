// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalWritePublicationMethod;

/// Verifies the public writer publication-method values remain distinct.
#[test]
fn test_local_write_publication_methods_are_distinct() {
    assert_ne!(
        LocalWritePublicationMethod::AtomicRename,
        LocalWritePublicationMethod::DirectAppend,
    );
}
