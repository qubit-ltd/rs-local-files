// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host and Rooted copies must report the same directory publication facts.

use std::fs;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use tempfile::tempdir;

/// Directory creation, merging, and replacement have distinct exact counters.
#[test]
fn test_directory_copy_statistics_match_publication_actions() {
    for rooted in [false, true] {
        for (existing, conflict, created, overwritten) in [
            ("missing", LocalCopyConflictPolicy::Overwrite, 1, 0),
            ("directory", LocalCopyConflictPolicy::Overwrite, 0, 1),
            ("directory", LocalCopyConflictPolicy::Skip, 0, 0),
            ("file", LocalCopyConflictPolicy::Overwrite, 1, 1),
        ] {
            let fixture = tempdir().expect("fixture should exist");
            fs::create_dir(fixture.path().join("source")).expect("source directory should exist");
            fs::write(fixture.path().join("source/payload"), b"new").expect("payload should exist");
            match existing {
                "directory" => fs::create_dir(fixture.path().join("target")).expect("target directory should exist"),
                "file" => fs::write(fixture.path().join("target"), b"old").expect("target file should exist"),
                _ => {}
            }
            let filesystem = if rooted {
                LocalFileSystem::rooted(fixture.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let source = if rooted {
                PathBuf::from("source")
            } else {
                fixture.path().join("source")
            };
            let target = if rooted {
                PathBuf::from("target")
            } else {
                fixture.path().join("target")
            };
            let outcome = filesystem
                .copy_with_options(
                    &source,
                    &target,
                    &LocalCopyOptions::new()
                        .with_tree_source()
                        .with_conflict(conflict)
                        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
                )
                .expect("directory copy should complete");
            assert_eq!(
                created,
                outcome.stats().directories(),
                "rooted={rooted}, existing={existing}"
            );
            assert_eq!(
                overwritten,
                outcome.stats().overwritten(),
                "rooted={rooted}, existing={existing}"
            );
            assert_eq!(1, outcome.stats().files());
            assert_eq!(3, outcome.stats().bytes());
            assert_eq!(0, outcome.stats().skipped());
            assert_eq!(
                b"new",
                fs::read(fixture.path().join("target/payload"))
                    .expect("payload should be published")
                    .as_slice()
            );
        }
    }
}
