// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::cell::Cell;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::os::windows::fs::OpenOptionsExt;

use tempfile::tempdir;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::open_root_directory_reader_with_reopen;
use crate::local::internal::rooted_directory_reader::RootedDirectoryReader;

const ENTRY_COUNT: usize = 700;
const NAME_UTF16_UNITS: usize = 180;

fn forced_fallback_reader(root: &File, fallback_count: &Cell<usize>) -> RootedDirectoryReader {
    open_root_directory_reader_with_reopen(root, |_| {
        fallback_count.set(fallback_count.get() + 1);
        Ok(None)
    })
    .expect("forced try_clone fallback should open a reader")
}

fn collect_names(reader: &mut RootedDirectoryReader) -> Vec<OsString> {
    let mut names = Vec::new();
    while let Some((name, _entry)) = reader.next_entry().expect("fallback enumeration should succeed") {
        names.push(name);
    }
    names
}

fn assert_complete_unique_set(names: &[OsString], expected: &BTreeSet<OsString>) {
    let actual = names.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), names.len(), "enumeration must not repeat entries");
    assert_eq!(expected, &actual, "enumeration must return the complete fixture");
}

#[test]
fn test_root_enumeration_fallback_keeps_independent_cursors() {
    let fixture = tempdir().expect("directory fixture should be created");
    let mut expected = BTreeSet::new();
    for index in 0..ENTRY_COUNT {
        let prefix = format!("entry-{index:04}-");
        let name = format!(
            "{prefix}{}",
            "x".repeat(NAME_UTF16_UNITS - prefix.encode_utf16().count())
        );
        fs::write(fixture.path().join(&name), []).expect("long-name fixture entry should be created");
        expected.insert(OsString::from(name));
    }
    assert!(ENTRY_COUNT * (NAME_UTF16_UNITS * 2 + 12) > 64 * 1024);

    let root = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(fixture.path())
        .expect("root directory handle should open");
    let fallback_count = Cell::new(0);

    // Exhaust B first, then drain A. Shared native enumeration state would
    // leave A empty or incomplete.
    let mut a = forced_fallback_reader(&root, &fallback_count);
    let mut b = forced_fallback_reader(&root, &fallback_count);
    let names_b = collect_names(&mut b);
    let names_a = collect_names(&mut a);
    assert_complete_unique_set(&names_a, &expected);
    assert_complete_unique_set(&names_b, &expected);

    // Alternate readers across native batches to expose cursor interference.
    let mut a = forced_fallback_reader(&root, &fallback_count);
    let mut b = forced_fallback_reader(&root, &fallback_count);
    let mut names_a = Vec::new();
    let mut names_b = Vec::new();
    loop {
        let a_entry = a.next_entry().expect("reader A should advance");
        let b_entry = b.next_entry().expect("reader B should advance");
        let a_done = a_entry.is_none();
        let b_done = b_entry.is_none();
        if let Some((name, _entry)) = a_entry {
            names_a.push(name);
        }
        if let Some((name, _entry)) = b_entry {
            names_b.push(name);
        }
        if a_done && b_done {
            break;
        }
        assert!(names_a.len() <= expected.len(), "reader A exceeded fixture size");
        assert!(names_b.len() <= expected.len(), "reader B exceeded fixture size");
    }
    assert_complete_unique_set(&names_a, &expected);
    assert_complete_unique_set(&names_b, &expected);

    // Dropping a partially advanced reader must not move B's cursor.
    let mut a = forced_fallback_reader(&root, &fallback_count);
    let mut b = forced_fallback_reader(&root, &fallback_count);
    let _ = a.next_entry().expect("reader A should yield its first entry");
    drop(a);
    let names_b = collect_names(&mut b);
    assert_complete_unique_set(&names_b, &expected);

    assert_eq!(6, fallback_count.get(), "each reader must take the forced fallback");
}
// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
