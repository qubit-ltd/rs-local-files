// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation support for local filesystem operations.

mod copy_dir;
mod dir_size_frame;
mod file_attribute_tag_info;
mod file_disposition_info;
mod file_io;
mod file_move;
mod io_result_context;
mod local_file_reader_inner;
mod local_file_writer_inner;
mod path_io_error;
mod path_operations;
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

pub(crate) use copy_dir::copy_dir_all_with_paths;
#[cfg(windows)]
pub(super) use file_attribute_tag_info::FileAttributeTagInfo;
#[cfg(windows)]
pub(super) use file_disposition_info::FileDispositionInfo;
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
pub(super) use rooted_atomic_write::{
    create_rooted_staged_file,
    existing_rooted_file_permissions,
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
    DEFAULT_TEMP_FILE_RETRIES,
    create_private_dir,
    create_temp_dir_in_dir,
    create_temp_file_in_dir,
};
