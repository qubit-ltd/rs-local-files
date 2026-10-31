// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Snapshot Debug output must omit the collected body.

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalSnapshotOptions;

#[test]
fn test_snapshot_debug_omits_body_and_keeps_explicit_access() {
    let directory = tempfile::tempdir().expect("create fixture");
    let filesystem = LocalFileSystem::host().expect("open Host");
    for size in [0_usize, 32, 1024 * 1024] {
        let bytes = vec![0xa5; size];
        let path = directory.path().join(format!("snapshot-{size}"));
        std::fs::write(&path, &bytes).expect("write fixture");
        let snapshot = filesystem
            .read_consistent_with_options(&path, &LocalSnapshotOptions::builder(size as u64).build())
            .expect("read snapshot");
        for output in [format!("{snapshot:?}"), format!("{snapshot:#?}")] {
            assert!(output.contains("LocalFileSnapshot"), "type is useful for diagnosis");
            assert!(output.contains("byte_len"), "length must remain observable");
            assert!(output.contains("before"), "before observation must remain");
            assert!(output.contains("after"), "after observation must remain");
            assert!(!output.contains("bytes:"), "body field must be omitted");
            assert!(!output.contains("165, 165"), "raw body bytes must be omitted");
            assert!(output.len() < 8192, "diagnosis must stay bounded for the fixture");
        }
        assert_eq!(snapshot.bytes(), bytes.as_slice());
        assert_eq!(snapshot.clone().into_bytes(), bytes);
    }
}
