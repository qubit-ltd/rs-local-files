// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
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
    LocalFilenames,
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

#[cfg(coverage)]
mod coverage_fault_process_tests;
mod current_dir_guard_tests;
#[cfg(target_os = "linux")]
mod file_owner_ex_tests;
mod filesystem_fixture_tests;
#[cfg(target_os = "freebsd")]
mod freebsd_acl_tests;
#[cfg(target_os = "macos")]
mod macos_acl_tests;
#[cfg(target_os = "linux")]
mod small_stack_process_tests;
#[cfg(target_os = "linux")]
mod source_read_lease_tests;
mod test_logger_tests;
#[cfg(windows)]
mod windows_security_tests;
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
))]
mod xattr_tests;

#[cfg(coverage)]
pub(super) use coverage_fault_process_tests::run_in_coverage_fault_process;
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
#[cfg(target_os = "freebsd")]
pub(super) use freebsd_acl_tests::{
    install_supported_test_acl,
    read_freebsd_acl_text,
};
#[cfg(target_os = "macos")]
pub(super) use macos_acl_tests::{
    read_macos_acl_text,
    set_current_user_read_acl,
};
#[cfg(target_os = "linux")]
pub(super) use small_stack_process_tests::run_in_small_stack_process;
#[cfg(target_os = "linux")]
pub(super) use source_read_lease_tests::{
    SourceReadLease,
    current_thread_cpu_time,
};
pub(super) use test_logger_tests::ensure_test_logger;
#[cfg(windows)]
pub(super) use windows_security_tests::{
    alternate_data_stream_path,
    clear_readonly_attribute,
    read_dacl_bytes,
    set_world_full_control_dacl,
};
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
))]
pub(super) use xattr_tests::{
    get_user_xattr,
    set_user_xattr,
};
