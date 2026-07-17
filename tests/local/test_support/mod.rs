// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
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
mod small_stack_process_tests;
#[cfg(target_os = "linux")]
mod source_read_lease_tests;
mod test_logger_tests;

pub(super) use current_dir_guard_tests::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
};
#[cfg(target_os = "linux")]
pub(super) use filesystem_fixture_tests::file_status_flags;
#[cfg(windows)]
pub(super) use filesystem_fixture_tests::path_with_interior_nul;
#[cfg(unix)]
pub(super) use filesystem_fixture_tests::{
    assert_fifo_open_is_rejected,
    create_fifo,
    short_temp_dir,
};
pub(super) use filesystem_fixture_tests::{
    count_atomic_temp_files,
    temp_dir,
};
#[cfg(target_os = "linux")]
pub(super) use small_stack_process_tests::run_in_small_stack_process;
#[cfg(target_os = "linux")]
pub(super) use source_read_lease_tests::SourceReadLease;
pub(super) use test_logger_tests::ensure_test_logger;
