// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Complete resolver calls must keep ordinary-path native work linear.

use std::fs;
use std::path::PathBuf;

use super::support::resolution_observation;
use crate::LocalFileOperation;
use crate::LocalSymlinkPolicy;
use crate::rooted::Root;
use crate::rooted_local_file_system::resolve_rooted_path;

/// Counts the actual fast-path operations, including accidental fallbacks.
#[test]
fn test_complete_resolver_uses_linear_ordinary_path_work() {
    let temporary = tempfile::tempdir().expect("fixture should exist");
    let root = Root::open(temporary.path()).expect("authority should open");
    for depth in [1, 8, 32, 64] {
        let mut path = PathBuf::new();
        for _ in 0..depth {
            path.push("d");
        }
        fs::create_dir_all(temporary.path().join(&path)).expect("deep directory should exist");
        path.push("payload");
        fs::write(temporary.path().join(&path), b"payload").expect("leaf should exist");
        for follow_final in [false, true] {
            resolution_observation::reset();
            let resolved = resolve_rooted_path(
                &root,
                &path,
                LocalSymlinkPolicy::FollowWithinScope,
                follow_final,
                LocalFileOperation::Metadata,
            )
            .expect("normal path should resolve");
            assert_eq!(path, resolved.as_path());
            let (metadata, opened, fallback) = resolution_observation::snapshot();
            assert_eq!(depth + usize::from(follow_final), metadata);
            assert_eq!(depth, opened);
            assert_eq!(0, fallback, "ordinary paths must not silently lose the fast path");
        }
    }
}
