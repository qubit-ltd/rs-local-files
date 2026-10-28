// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use crate::LocalFileSystem;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::policy::LocalOwnerAccessPolicy;

#[test]
fn explicit_entry_limit_can_raise_owner_policy_default_ceiling() {
    const ENTRY_COUNT: usize = 257;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new().with_owner_access_policy(policy))
        .expect("stage should be created");

    for index in 0..ENTRY_COUNT {
        let name = format!("entry-{index}");
        stage
            .write_file_new(Path::new(&name), &mut &b"x"[..], 1)
            .expect("entry should be written");
    }

    let outcome = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new().with_max_entries(ENTRY_COUNT))
        .expect("explicit entry limit should permit all entries");

    assert_eq!(outcome.owner_access_policy(), Some(&policy));
}
