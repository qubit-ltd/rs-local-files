// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation support for local filesystem operations.
// qubit-style: allow coverage-cfg

mod atomic_file_install;
mod atomic_install_recovery;
#[cfg(unix)]
mod atomic_metadata;
#[cfg(unix)]
mod atomic_namespace_race;
mod atomic_staging_state;
mod copy_dir;
#[cfg(coverage)]
pub(crate) mod coverage_fault;
mod dir_size_frame;
mod directory_identity;
mod file_io;
mod file_move;
mod io_result_context;
mod local_file_reader_inner;
mod local_file_writer_inner;
#[cfg(unix)]
mod opened_atomic_destination;
mod path_io_error;
mod path_operations;
#[cfg(unix)]
mod rooted_atomic_install;
#[cfg(unix)]
mod rooted_atomic_namespace_race;
#[cfg(unix)]
mod rooted_atomic_write;
#[cfg(unix)]
mod rooted_file_io;
#[cfg(unix)]
mod rooted_io_result;
#[cfg(unix)]
mod rooted_parent;
#[cfg(unix)]
mod rooted_parent_mode;
#[cfg(unix)]
mod rooted_staged_file;
#[cfg(unix)]
mod rooted_staging_retry;
mod staged_file;
mod temp_entry;
#[cfg(unix)]
mod unix_nonblocking;
#[cfg(unix)]
mod unix_stat;

pub(crate) use atomic_file_install::install_atomic_file;
pub(crate) use atomic_install_recovery::{
    AtomicInstallRecovery,
    recover_atomic_install_error,
};
#[cfg(unix)]
pub(crate) use atomic_metadata::preserve_atomic_metadata;
#[cfg(unix)]
pub(crate) use atomic_namespace_race::verify_atomic_destination_identity;
pub(crate) use atomic_staging_state::AtomicStagingState;
pub(crate) use copy_dir::copy_dir_all_with_paths;
pub(crate) use file_io::{
    open_reader_path,
    open_writer_path,
};
pub(crate) use file_move::{
    move_directory_without_replacing,
    move_file_without_replacing,
    parent_dir_for,
    replace_file,
    sync_parent_dir,
};
pub(super) use local_file_reader_inner::LocalFileReaderInner;
pub(super) use local_file_writer_inner::LocalFileWriterInner;
#[cfg(unix)]
pub(super) use opened_atomic_destination::open_rooted_atomic_destination;
#[cfg(unix)]
pub(crate) use opened_atomic_destination::{
    OpenedAtomicDestination,
    open_atomic_destination,
};
pub(crate) use path_operations::{
    absolute_path,
    add_path_context,
    clean_dir_path,
    dir_size_path,
    ensure_dir_path,
    ensure_parent_path,
    ensure_parent_path_with_sync_dirs,
    remove_any_path,
};
#[cfg(unix)]
pub(super) use rooted_atomic_install::install_rooted_atomic_file;
#[cfg(unix)]
pub(super) use rooted_atomic_namespace_race::verify_rooted_atomic_destination_identity;
#[cfg(unix)]
pub(super) use rooted_atomic_write::{
    create_rooted_staged_file,
    inspect_rooted_atomic_destination,
};
#[cfg(unix)]
pub(super) use rooted_file_io::open_rooted_parent;
#[cfg(unix)]
pub(crate) use rooted_file_io::{
    open_root_directory,
    open_rooted_reader,
    open_rooted_writer,
};
#[cfg(unix)]
pub(super) use rooted_parent_mode::RootedParentMode;
#[cfg(unix)]
pub(super) use rooted_staged_file::RootedStagedFile;
pub(crate) use staged_file::StagedFile;
pub(crate) use temp_entry::{
    DEFAULT_TEMP_ENTRY_RETRIES,
    create_private_dir,
    create_temp_dir_in_dir,
    create_temp_file_in_dir,
};
#[cfg(unix)]
pub(crate) use unix_nonblocking::{
    clear_nonblocking,
    open_with_nonblocking_retry,
};
