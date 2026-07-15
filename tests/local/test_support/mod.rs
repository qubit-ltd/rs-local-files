// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Shared fixtures and helpers for local-filesystem integration tests.

pub(super) use qubit_local_files::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};
pub(super) use std::fs;
pub(super) use std::io::{
    ErrorKind,
    Read,
    Seek,
    SeekFrom,
    Write,
};
#[cfg(unix)]
pub(super) use std::os::unix::fs::PermissionsExt;

mod current_dir_guard_tests;
mod filesystem_fixture_tests;
#[cfg(target_os = "linux")]
mod source_read_lease_tests;
mod test_logger_tests;

pub(super) use current_dir_guard_tests::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
};
#[cfg(windows)]
pub(super) use filesystem_fixture_tests::path_with_interior_nul;
#[cfg(unix)]
pub(super) use filesystem_fixture_tests::short_temp_dir;
pub(super) use filesystem_fixture_tests::{
    count_atomic_temp_files,
    temp_dir,
};
#[cfg(target_os = "linux")]
pub(super) use source_read_lease_tests::SourceReadLease;
pub(super) use test_logger_tests::ensure_test_logger;
