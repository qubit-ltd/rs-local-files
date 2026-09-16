// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalAtomicWriteOptions`.

use std::time::Duration;

use crate::local::LocalAtomicPublicationMode;
use crate::local::LocalAtomicWriteOptions;
use crate::policy::LocalDurabilityRequirement;

#[test]
fn test_local_atomic_write_options_builders_update_accessible_policies() {
    let options = LocalAtomicWriteOptions::new()
        .with_create_parent()
        .with_open_retry_timeout(Duration::from_secs(1))
        .with_create_new()
        .with_durability(LocalDurabilityRequirement::NotRequired);
    assert!(options.creates_parent());
    assert_eq!(options.open_retry_timeout(), Some(Duration::from_secs(1)));
    assert_eq!(options.durability(), LocalDurabilityRequirement::NotRequired);
    assert!(!options.replaces_target_symlink());
    assert_eq!(options.publication_mode(), LocalAtomicPublicationMode::CreateNew);
    assert_eq!(LocalAtomicWriteOptions::default(), LocalAtomicWriteOptions::new());
}
