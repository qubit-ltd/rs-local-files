// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private local file reader and writer construction.

use std::fs::{
    self,
    File,
    OpenOptions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::Path;

use crate::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalFileReader,
    LocalFileWriter,
};

use super::path_operations::{
    add_path_context,
    ensure_parent_path,
};

/// Creates the canonical reader error for a non-file target.
///
/// # Parameters
/// - `path`: Path that did not resolve to an opened regular file.
///
/// # Returns
/// Invalid-input error describing the rejected path.
fn opened_path_not_file_error(path: &Path) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("opened path is not a file: {}", path.display()),
    )
}

/// Opens a file reader with the supplied options.
///
/// # Parameters
/// - `path`: File path to open.
/// - `options`: Read options controlling buffering.
///
/// # Returns
/// A local file reader.
///
/// # Errors
/// Returns an I/O error when `path` cannot be inspected or opened, or when the
/// target is not a file.
pub(crate) fn open_reader_path(
    path: &Path,
    options: FileReadOptions,
) -> Result<LocalFileReader> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            if fs::metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
                return Err(opened_path_not_file_error(path));
            }
            return Err(add_path_context(error, "open file", path));
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(opened_path_not_file_error(path));
    }
    Ok(LocalFileReader::from_file(file, options.buffering))
}

/// Opens a file writer with the supplied options.
///
/// # Parameters
/// - `path`: File path to open.
/// - `options`: Write options controlling parent creation, write mode, and
///   buffering.
///
/// # Returns
/// A local file writer.
///
/// # Errors
/// Returns an I/O error when parent directories cannot be created or the file
/// cannot be opened with the requested mode.
pub(crate) fn open_writer_path(
    path: &Path,
    options: FileWriteOptions,
) -> Result<LocalFileWriter> {
    if options.create_parent {
        ensure_parent_path(path)?;
    }
    let mut open_options = OpenOptions::new();
    match options.mode {
        FileWriteMode::OpenExistingAtStart => {
            open_options.write(true);
        }
        FileWriteMode::CreateNew => {
            open_options.write(true).create_new(true);
        }
        FileWriteMode::CreateOrTruncate => {
            open_options.write(true).create(true).truncate(true);
        }
        FileWriteMode::AppendExisting => {
            open_options.append(true);
        }
        FileWriteMode::AppendOrCreate => {
            open_options.append(true).create(true);
        }
    }
    let file = open_options
        .open(path)
        .map_err(|error| add_path_context(error, "open file writer", path))?;
    Ok(LocalFileWriter::from_file(file, options.buffering))
}
