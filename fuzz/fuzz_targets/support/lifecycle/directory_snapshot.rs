// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded, non-following snapshots of published directory trees.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use super::MAX_DEPTH;
use super::MAX_ENTRIES;
use super::MAX_SNAPSHOT_BYTES;
use super::SnapshotEntry;

/// A bounded, non-following snapshot of a published directory tree.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct DirectorySnapshot {
    /// Relative entry paths paired with their kind and relevant bytes.
    entries: BTreeMap<PathBuf, SnapshotEntry>,
}

impl DirectorySnapshot {
    /// Captures the tree rooted at `root` within the harness entry and
    /// byte budgets.
    pub(super) fn capture(root: &Path) -> Self {
        /// Records one entry and recursively visits directory children
        /// without following symlinks.
        fn visit(path: &Path, relative: &Path, entries: &mut BTreeMap<PathBuf, SnapshotEntry>, byte_count: &mut usize) {
            assert!(
                relative.components().count() <= MAX_DEPTH,
                "snapshot exceeded its depth budget"
            );
            assert!(entries.len() < MAX_ENTRIES, "snapshot exceeded its entry budget");
            let metadata = fs::symlink_metadata(path).expect("snapshot entry metadata");
            let snapshot_entry = if metadata.file_type().is_symlink() {
                let target = fs::read_link(path).expect("read snapshot link text");
                *byte_count = byte_count
                    .checked_add(relative.as_os_str().len())
                    .and_then(|size| size.checked_add(target.as_os_str().len()))
                    .expect("snapshot byte budget arithmetic");
                assert!(*byte_count <= MAX_SNAPSHOT_BYTES, "snapshot exceeded its byte budget");
                SnapshotEntry::Symlink(target)
            } else if metadata.is_dir() {
                *byte_count = byte_count
                    .checked_add(relative.as_os_str().len())
                    .expect("snapshot byte budget arithmetic");
                assert!(*byte_count <= MAX_SNAPSHOT_BYTES, "snapshot exceeded its byte budget");
                SnapshotEntry::Directory
            } else if metadata.is_file() {
                let file_size = usize::try_from(metadata.len()).expect("snapshot file size fits usize");
                *byte_count = byte_count
                    .checked_add(relative.as_os_str().len())
                    .and_then(|size| size.checked_add(file_size))
                    .expect("snapshot byte budget arithmetic");
                assert!(*byte_count <= MAX_SNAPSHOT_BYTES, "snapshot exceeded its byte budget");
                SnapshotEntry::File(fs::read(path).expect("read snapshot file"))
            } else {
                panic!("unsupported entry in fuzz-owned directory: {path:?}");
            };
            entries.insert(relative.to_path_buf(), snapshot_entry);

            if metadata.is_dir() {
                for child in fs::read_dir(path).expect("read snapshot directory") {
                    let child = child.expect("read snapshot directory entry");
                    let child_path = child.path();
                    let child_relative = relative.join(child.file_name());
                    visit(&child_path, &child_relative, entries, byte_count);
                }
            }
        }

        let mut entries = BTreeMap::new();
        let mut byte_count = 0;
        visit(root, Path::new(""), &mut entries, &mut byte_count);
        Self { entries }
    }

    /// Asserts that `root` has the exact relative paths, entry kinds,
    /// symlink text, and file bytes recorded by this snapshot.
    pub(super) fn assert_matches(&self, root: &Path) {
        assert_eq!(self, &Self::capture(root), "published directory tree changed: {root:?}");
    }
}
