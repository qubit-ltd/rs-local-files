// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalCopyDirOptions`.

use std::time::Duration;

use crate::LocalCopyConflictPolicy;
use crate::LocalCopyDirOptions;
use crate::LocalCopyTypeConflictPolicy;
use crate::LocalDurabilityRequirement;
use crate::LocalSymlinkPolicy;

#[test]
fn test_local_copy_dir_options_builders_update_every_policy() {
    let options = LocalCopyDirOptions::new()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .preserve_permissions()
        .with_open_retry_timeout(Duration::from_secs(1))
        .with_durability(LocalDurabilityRequirement::Required);
    assert_eq!(
        options.conflict_policy(),
        LocalCopyConflictPolicy::Overwrite
    );
    assert_eq!(
        options.type_conflict_policy(),
        LocalCopyTypeConflictPolicy::Replace
    );
    assert_eq!(
        options.symlink_policy(),
        LocalSymlinkPolicy::FollowWithinScope
    );
    assert!(options.preserves_permissions());
    assert_eq!(options.open_retry_timeout(), Some(Duration::from_secs(1)));
    assert_eq!(options.durability(), LocalDurabilityRequirement::Required);
    assert_eq!(LocalCopyDirOptions::default(), LocalCopyDirOptions::new());
}
