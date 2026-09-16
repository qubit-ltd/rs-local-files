// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic library probe complexity, independent of filesystem timing.
#![cfg(feature = "test-support")]

use qubit_local_files::LocalFileSystem;
use qubit_local_files::policy::LocalSymlinkPolicy;
use qubit_local_files::test_support::host_metadata_probe_counts;
use qubit_local_files::test_support::reset_host_metadata_probe_counts;

/// Default Host metadata performs one final query and no prefix probes at every
/// depth.
#[test]
fn test_host_metadata_query_count_is_independent_of_depth() {
    let dir = tempfile::tempdir().expect("isolated fixture");
    let host = LocalFileSystem::host().expect("host filesystem");
    for depth in [1, 32, 128] {
        let mut path = dir.path().to_path_buf();
        for _ in 0..depth {
            path.push("d");
        }
        std::fs::create_dir_all(&path).expect("deep directory fixture");
        path.push("file");
        std::fs::write(&path, b"payload").expect("file fixture");
        reset_host_metadata_probe_counts();
        assert_eq!(host.metadata(&path).expect("Host metadata").len(), 7);
        let (prefix_probes, final_queries) = host_metadata_probe_counts();
        assert_eq!(prefix_probes, 0, "default metadata must not inspect prefixes");
        assert_eq!(final_queries, 1);
    }
}

/// Reject-policy traversal is visible to the same probe instrument.
#[test]
fn test_reject_policy_records_required_prefix_probes() {
    let dir = tempfile::tempdir().expect("isolated fixture");
    let mut host = LocalFileSystem::host().expect("host filesystem");
    host.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("reject policy");
    // Resolve the fixture's own platform aliases before imposing Reject.
    let path = std::fs::canonicalize(dir.path())
        .expect("physical fixture path")
        .join("file");
    std::fs::write(&path, b"payload").expect("file fixture");
    reset_host_metadata_probe_counts();
    let _ = host.metadata(&path).expect("metadata without links");
    let (prefix_probes, final_queries) = host_metadata_probe_counts();
    assert!(prefix_probes > 0, "the instrument must observe policy probes");
    assert_eq!(final_queries, 1);
}
