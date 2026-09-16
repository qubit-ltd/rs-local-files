// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Explicit metadata selection on local writer options.

use qubit_local_files::options::LocalWriteMetadataPolicy;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;

/// Selecting replacement metadata must not change unrelated writer behavior.
#[test]
fn test_metadata_policy_defaults_to_preservation() {
    let original = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    assert_eq!(original.metadata_policy(), LocalWriteMetadataPolicy::PreserveExisting);
    let selected = original.with_metadata_policy(LocalWriteMetadataPolicy::UseStaging);
    assert_eq!(selected.metadata_policy(), LocalWriteMetadataPolicy::UseStaging);
    assert_eq!(selected.mode(), original.mode());
    assert_eq!(selected.atomicity(), original.atomicity());
    assert_eq!(selected.durability(), original.durability());
    assert_eq!(selected.creates_parent(), original.creates_parent());
    assert_eq!(selected.open_retry_timeout(), original.open_retry_timeout());
}
