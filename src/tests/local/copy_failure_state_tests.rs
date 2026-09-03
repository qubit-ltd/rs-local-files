// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private publication-state mapping tests.

use crate::local::LocalCopyDirStage;
use crate::outcome::LocalCopyFailureState;
use crate::outcome::LocalCopyStats;
use crate::outcome::copy_failure_state;

/// Verifies symbolic-link publication stages retain exact recovery states.
#[test]
fn test_symbolic_link_publication_stages_map_exact_states() {
    let stats = LocalCopyStats::default();

    assert_eq!(
        LocalCopyFailureState::Unchanged,
        copy_failure_state(LocalCopyDirStage::PublishSymlinkUnchanged, stats),
    );
    assert_eq!(
        LocalCopyFailureState::PartiallyPublished,
        copy_failure_state(LocalCopyDirStage::PublishSymlinkPartially, stats),
    );
    assert_eq!(
        LocalCopyFailureState::Indeterminate,
        copy_failure_state(LocalCopyDirStage::PublishSymlinkIndeterminate, stats),
    );
}
