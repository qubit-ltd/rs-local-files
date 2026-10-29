// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Windows regression coverage for independent Rooted directory enumeration.

#![cfg(windows)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalDirectoryWalker;
use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;

const ROOT_ENTRY_COUNT: usize = 1_500;
const ROOT_NAME_UTF16_UNITS: usize = 180;

fn collect_names(walker: LocalDirectoryWalker) -> Vec<PathBuf> {
    walker
        .map(|entry| {
            entry
                .expect("directory entry should be readable")
                .relative_path()
                .to_path_buf()
        })
        .collect()
}

fn assert_complete_unique_set(names: &[PathBuf], expected: &BTreeSet<PathBuf>) {
    let actual = names.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), names.len(), "enumeration must not repeat entries");
    assert_eq!(expected, &actual, "enumeration must return the complete fixture");
}

fn root_walkers(rooted: &LocalFileSystem) -> (LocalDirectoryWalker, LocalDirectoryWalker) {
    let clone = rooted.clone();
    (
        rooted.list(Path::new("/")).expect("first root walker should open"),
        clone.list(Path::new("/")).expect("cloned root walker should open"),
    )
}

#[test]
fn rooted_root_enumerators_have_independent_cursors_across_native_batches() {
    let fixture = tempdir().expect("temporary root should be created");
    let descendant_path = fixture.path().join("descendant");
    fs::create_dir(&descendant_path).expect("descendant fixture should be created");

    let mut expected_root = BTreeSet::new();
    expected_root.insert(PathBuf::from("descendant"));
    for index in 0..ROOT_ENTRY_COUNT {
        let mut name = format!("entry-{index:04}-");
        name.push_str(&"x".repeat(ROOT_NAME_UTF16_UNITS - name.encode_utf16().count()));
        fs::write(fixture.path().join(&name), []).expect("root fixture file should be created");
        expected_root.insert(PathBuf::from(name));
    }
    let mut expected_descendant = BTreeSet::new();
    for index in 0..32 {
        let name = format!("child-{index:03}");
        fs::write(descendant_path.join(&name), []).expect("descendant fixture file should be created");
        expected_descendant.insert(PathBuf::from(name));
    }

    // Each native entry needs at least the UTF-16 name plus its fixed record
    // header, so this immutable fixture necessarily exceeds one 64 KiB batch.
    let estimated_native_record_bytes = ROOT_ENTRY_COUNT * (ROOT_NAME_UTF16_UNITS * 2 + 12);
    assert!(estimated_native_record_bytes > 64 * 1024);

    let rooted = LocalFileSystem::rooted(fixture.path()).expect("root authority should open");

    // Let B exhaust the shared authority first, then verify A can still drain.
    let (mut first, second) = root_walkers(&rooted);
    let first_item = first
        .next()
        .expect("first walker should yield an entry")
        .expect("entry should be readable");
    let second_names = collect_names(second);
    let mut first_names = vec![first_item.relative_path().to_path_buf()];
    first_names.extend(collect_names(first));
    assert_complete_unique_set(&first_names, &expected_root);
    assert_complete_unique_set(&second_names, &expected_root);

    // Then alternate advancement to expose cursor sharing during overlap.
    let (mut first, mut second) = root_walkers(&rooted);
    let mut first_names = Vec::new();
    let mut second_names = Vec::new();
    loop {
        let first_entry = first.next();
        let second_entry = second.next();
        let first_done = first_entry.is_none();
        let second_done = second_entry.is_none();
        if let Some(entry) = first_entry {
            first_names.push(
                entry
                    .expect("first entry should be readable")
                    .relative_path()
                    .to_path_buf(),
            );
        }
        if let Some(entry) = second_entry {
            second_names.push(
                entry
                    .expect("second entry should be readable")
                    .relative_path()
                    .to_path_buf(),
            );
        }
        if first_done && second_done {
            break;
        }
        assert!(
            first_names.len() <= expected_root.len(),
            "first walker exceeded fixture size"
        );
        assert!(
            second_names.len() <= expected_root.len(),
            "second walker exceeded fixture size"
        );
    }
    assert_complete_unique_set(&first_names, &expected_root);
    assert_complete_unique_set(&second_names, &expected_root);

    let clone = rooted.clone();
    let mut descendant_a = rooted
        .list(Path::new("/descendant"))
        .expect("first descendant walker should open");
    let descendant_b = clone
        .list(Path::new("/descendant"))
        .expect("cloned descendant walker should open");
    let first_descendant = descendant_a
        .next()
        .expect("first descendant should have entries")
        .expect("first descendant entry should be readable")
        .relative_path()
        .to_path_buf();
    let mut descendant_a_names = vec![first_descendant];
    descendant_a_names.extend(collect_names(descendant_a));
    let descendant_b_names = collect_names(descendant_b);
    assert_complete_unique_set(&descendant_a_names, &expected_descendant);
    assert_complete_unique_set(&descendant_b_names, &expected_descendant);
}
