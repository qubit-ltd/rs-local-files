// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local-filesystem fixture construction and inspection.

use std::fs;
use std::path::{
    Path,
    PathBuf,
};
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

/// Counter used to keep per-process temporary fixture names unique.
static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Creates an empty, uniquely named temporary fixture directory.
///
/// Any stale fixture at the generated path is removed before creation.
///
/// # Parameters
///
/// * `name` - Human-readable test name included in the generated path.
///
/// # Returns
///
/// Path to the created fixture directory.
///
/// # Panics
///
/// Panics when the fixture directory cannot be created.
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "qubit-local-files-local-tests-{}-{name}-{id}",
        std::process::id()
    ));
    drop(fs::remove_dir_all(&path));
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

#[cfg(unix)]
/// Creates a short-path Unix fixture directory under `/tmp`.
///
/// Any stale fixture at the generated path is removed before creation.
///
/// # Parameters
///
/// * `name` - Human-readable test name included in the generated path.
///
/// # Returns
///
/// Path to the created fixture directory.
///
/// # Panics
///
/// Panics when the fixture directory cannot be created.
pub(crate) fn short_temp_dir(name: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path =
        PathBuf::from(format!("/tmp/qio-{}-{name}-{id}", std::process::id()));
    drop(fs::remove_dir_all(&path));
    fs::create_dir_all(&path).expect("short temp dir should be created");
    path
}

#[cfg(windows)]
/// Builds a Windows path whose final component contains an interior UTF-16 NUL.
///
/// # Parameters
///
/// * `parent` - Parent directory for the malformed component.
/// * `prefix` - Visible component prefix placed before the NUL.
///
/// # Returns
///
/// A path ending in `prefix`, an interior NUL, and the character `x`.
pub(crate) fn path_with_interior_nul(parent: &Path, prefix: &str) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let mut units: Vec<u16> = prefix.encode_utf16().collect();
    units.extend([0, u16::from(b'x')]);
    parent.join(OsString::from_wide(&units))
}

/// Counts atomic-write staging entries in a fixture directory.
///
/// # Parameters
///
/// * `dir` - Directory to scan.
///
/// # Returns
///
/// Number of direct children whose names start with `.atomic-write-`.
///
/// # Panics
///
/// Panics when the directory cannot be read.
pub(crate) fn count_atomic_temp_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".atomic-write-")
        })
        .count()
}
