// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation support for local filesystem operations.

mod atomic_write;
mod copy_dir;
mod file_io;
mod file_move;
mod path_io_error;
mod path_operations;
mod staged_file;
mod temp_entry;

pub(crate) use atomic_write::{
    atomic_write_bytes_path,
    atomic_write_with_path,
};
pub(crate) use copy_dir::copy_dir_all_with_paths;
pub(crate) use file_io::{
    open_reader_path,
    open_writer_path,
};
pub(crate) use file_move::{
    move_directory_without_replacing,
    move_file_without_replacing,
    replace_file,
};
pub(crate) use path_operations::{
    clean_dir_path,
    dir_size_path,
    ensure_dir_path,
    ensure_parent_path,
    remove_any_path,
};
pub(crate) use staged_file::StagedFile;
pub(crate) use temp_entry::{
    DEFAULT_TEMP_FILE_RETRIES,
    create_private_dir,
    create_temp_dir_in_dir,
    create_temp_file_in_dir,
};
