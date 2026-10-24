// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use crate::LocalFileSystem;
use crate::path::LocalFileNames;
use crate::path::LocalFileSystemScope;
use crate::path::LocalPaths;

#[test]
fn test_local_path_and_filename_policy_accessors_and_generation() {
    let host = LocalPaths::host();
    assert_eq!(LocalFileSystemScope::Host, host.scope());
    assert_eq!(None, host.file_names().max_component_bytes());

    let rooted = LocalPaths::rooted();
    assert_eq!(LocalFileSystemScope::Rooted, rooted.scope());

    let names = LocalFileNames::portable();
    assert_eq!(None, names.max_component_bytes());
    names
        .validate(OsStr::new("report.txt"))
        .expect("portable filename should validate");

    let bounded = names
        .with_max_component_bytes(64)
        .expect("positive component limit should be accepted");
    assert_eq!(Some(64), bounded.max_component_bytes());
    assert!(bounded.random_name().is_ok());
    let generated = bounded
        .random_name_with(Some(OsStr::new("prefix-")), Some(OsStr::new(".txt")))
        .expect("random name with native affixes should validate");
    assert!(generated.to_string_lossy().starts_with("prefix-"));
    assert!(generated.to_string_lossy().ends_with(".txt"));

    let unbounded = bounded.without_max_component_bytes();
    assert_eq!(None, unbounded.max_component_bytes());
}

#[test]
fn test_reader_exposes_metadata_captured_from_its_open_handle() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("payload should be written");

    let reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader(Path::new(&path))
        .expect("reader should open");

    assert_eq!(7, reader.metadata().len());
}
