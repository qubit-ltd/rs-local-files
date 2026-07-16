// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation support for local filesystem operations.

mod copy_dir;
mod file_attribute_tag_info;
mod file_disposition_info;
mod file_io;
mod file_move;
mod path_io_error;
mod path_operations;
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
pub(crate) use path_operations::{
    add_path_context,
    clean_dir_path,
    dir_size_path,
    ensure_dir_path,
    ensure_parent_path,
    ensure_parent_path_with_sync_dirs,
    remove_any_path,
};
pub(crate) use staged_file::StagedFile;
pub(crate) use temp_entry::{
    DEFAULT_TEMP_FILE_RETRIES,
    create_private_dir,
    create_temp_dir_in_dir,
    create_temp_file_in_dir,
};
