// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

pub(super) use std::fs;
pub(super) use std::io::{Error, ErrorKind, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
pub(super) use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};

pub(super) use qubit_local_files::{
    FileBuffering, FileReadOptions, FileWriteMode, FileWriteOptions, LocalPersistOptions,
    LocalTempDir, LocalTempFile,
};

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(super) static CURRENT_DIR_LOCK: Mutex<()> = Mutex::new(());
static LOGGER_INIT: Once = Once::new();

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, _record: &log::Record<'_>) {}

    fn flush(&self) {}
}

static TEST_LOGGER: TestLogger = TestLogger;

pub(super) fn ensure_test_logger() {
    LOGGER_INIT.call_once(|| {
        if log::set_logger(&TEST_LOGGER).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    });
}

pub(super) fn temp_dir(name: &str) -> PathBuf {
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
pub(super) fn short_temp_dir(name: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = PathBuf::from(format!("/tmp/qio-{}-{name}-{id}", std::process::id()));
    drop(fs::remove_dir_all(&path));
    fs::create_dir_all(&path).expect("short temp dir should be created");
    path
}

pub(super) fn count_atomic_temp_files(dir: &std::path::Path) -> usize {
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

pub(super) struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    pub(super) fn change_to(path: &std::path::Path) -> Self {
        let original = std::env::current_dir().expect("current dir should be readable");
        std::env::set_current_dir(path).expect("current dir should be changed");
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        drop(std::env::set_current_dir(&self.original));
    }
}
