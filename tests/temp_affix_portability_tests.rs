// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use tempfile::tempdir;

#[test]
fn test_temp_affixes_validate_complete_portable_name_before_side_effects() {
    for rooted in [false, true] {
        for directory_entry in [false, true] {
            for (prefix, suffix) in [("", ":"), ("", "name."), ("", "name "), ("CON.", "")] {
                let fixture = tempdir().expect("fixture should be created");
                let missing = fixture.path().join("missing");
                let filesystem = if rooted {
                    LocalFileSystem::rooted(fixture.path()).expect("root authority should open")
                } else {
                    LocalFileSystem::host().expect("host authority should open")
                };
                let parent = if rooted {
                    Path::new("/missing")
                } else {
                    missing.as_path()
                };
                let error = if directory_entry {
                    filesystem
                        .create_temp_directory_with_options(
                            &LocalTempDirectoryOptions::new()
                                .with_parent(parent)
                                .with_prefix(prefix)
                                .with_suffix(suffix)
                                .with_create_parent(),
                        )
                        .expect_err("nonportable final component must be rejected")
                } else {
                    filesystem
                        .create_temp_file_with_options(
                            &LocalTempFileOptions::new()
                                .with_parent(parent)
                                .with_prefix(prefix)
                                .with_suffix(suffix)
                                .with_create_parent(),
                        )
                        .expect_err("nonportable final component must be rejected")
                };
                assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
                if !rooted {
                    assert!(!missing.exists(), "validation must precede parent creation");
                }
            }
        }
    }
}

#[test]
fn test_temp_affixes_allow_legal_fragments_and_default_names() {
    let fixture = tempdir().expect("fixture should be created");
    for rooted in [false, true] {
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("root authority should open")
        } else {
            LocalFileSystem::host().expect("host authority should open")
        };
        let parent = if rooted { Path::new("/") } else { fixture.path() };
        for directory_entry in [false, true] {
            for (prefix, suffix) in [
                (Some("tmp."), None),
                (None, Some(".tmp")),
                (Some(""), Some("")),
                (None, None),
            ] {
                if directory_entry {
                    let options = LocalTempDirectoryOptions::new().with_parent(parent);
                    let options = if let Some(prefix) = prefix {
                        options.with_prefix(prefix)
                    } else {
                        options
                    };
                    let options = if let Some(suffix) = suffix {
                        options.with_suffix(suffix)
                    } else {
                        options
                    };
                    drop(
                        filesystem
                            .create_temp_directory_with_options(&options)
                            .expect("valid directory affixes should be accepted"),
                    );
                } else {
                    let options = LocalTempFileOptions::new().with_parent(parent);
                    let options = if let Some(prefix) = prefix {
                        options.with_prefix(prefix)
                    } else {
                        options
                    };
                    let options = if let Some(suffix) = suffix {
                        options.with_suffix(suffix)
                    } else {
                        options
                    };
                    drop(
                        filesystem
                            .create_temp_file_with_options(&options)
                            .expect("valid file affixes should be accepted"),
                    );
                }
            }
        }
    }
    assert_eq!(
        0,
        fs::read_dir(fixture.path())
            .expect("parent should remain readable")
            .count()
    );
}
