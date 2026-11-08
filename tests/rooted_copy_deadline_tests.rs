// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyOptions;

#[test]
fn test_rooted_copy_with_and_without_positive_deadline_succeeds() {
    for deadline in [None, Some(Duration::from_secs(60))] {
        let fixture = tempfile::tempdir().expect("fixture should exist");
        fs::write(fixture.path().join("source"), b"payload").expect("source should exist");
        let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
        let mut options = LocalCopyOptions::new();
        if let Some(deadline) = deadline {
            options = options.with_deadline(deadline);
        }

        let outcome = filesystem
            .copy_with_options(Path::new("source"), Path::new("target"), &options)
            .expect("ordinary rooted copy should succeed");

        assert_eq!(1, outcome.stats().files());
        assert_eq!(
            b"payload",
            fs::read(fixture.path().join("target"))
                .expect("copied target should exist")
                .as_slice()
        );
    }
}
