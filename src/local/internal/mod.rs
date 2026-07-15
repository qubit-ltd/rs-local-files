// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation support for local filesystem operations.

mod local_file_operations;
mod path_io_error;
mod staged_file;

pub(crate) use local_file_operations::{
    LocalFileOperations,
    create_private_dir,
    create_temp_dir_in_dir,
    create_temp_file_in_dir,
    move_directory_without_replacing,
    move_file_without_replacing,
    replace_file,
};
pub(crate) use staged_file::StagedFile;
