// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Concrete local filesystem APIs and their private implementation.

mod file_buffering;
mod file_read_options;
mod file_write_mode;
mod file_write_options;
mod internal;
mod local_atomic_commit_error;
mod local_atomic_destination_state;
mod local_atomic_write_error;
mod local_atomic_write_options;
mod local_atomic_write_stage;
mod local_atomic_writer;
mod local_copy_conflict_policy;
mod local_copy_dir_error;
mod local_copy_dir_options;
mod local_copy_dir_stage;
mod local_copy_dir_stats;
mod local_copy_type_conflict_policy;
mod local_file_reader;
mod local_file_writer;
mod local_filenames;
mod local_files;
mod local_persist_error;
mod local_persist_options;
mod local_persist_stage;
mod local_relative_path;
mod local_root;
mod local_root_atomic_writer;
mod local_temp_dir;
mod local_temp_file;

pub(crate) use internal::{
    clean_dir_path,
    copy_dir_all_with_paths,
    dir_size_path,
    ensure_dir_path,
    ensure_parent_path,
    open_native_reader_path,
    open_native_writer_path,
    remove_any_path,
    validate_portable_file_name_impl,
};
#[cfg(unix)]
pub(crate) use internal::{
    open_root_directory,
    open_rooted_native_reader,
    open_rooted_native_writer,
    read_rooted_symlink_metadata,
};

pub use file_buffering::FileBuffering;
pub use file_read_options::FileReadOptions;
pub use file_write_mode::FileWriteMode;
pub use file_write_options::FileWriteOptions;
pub use local_atomic_commit_error::LocalAtomicCommitError;
pub use local_atomic_destination_state::LocalAtomicDestinationState;
pub use local_atomic_write_error::LocalAtomicWriteError;
pub use local_atomic_write_options::LocalAtomicWriteOptions;
pub use local_atomic_write_stage::LocalAtomicWriteStage;
pub use local_atomic_writer::LocalAtomicWriter;
pub use local_copy_conflict_policy::LocalCopyConflictPolicy;
pub use local_copy_dir_error::LocalCopyDirError;
pub use local_copy_dir_options::LocalCopyDirOptions;
pub use local_copy_dir_stage::LocalCopyDirStage;
pub use local_copy_dir_stats::LocalCopyDirStats;
pub use local_copy_type_conflict_policy::LocalCopyTypeConflictPolicy;
pub use local_file_reader::LocalFileReader;
pub use local_file_writer::LocalFileWriter;
pub use local_filenames::LocalFilenames;
pub use local_files::LocalFiles;
pub use local_persist_error::LocalPersistError;
pub use local_persist_options::LocalPersistOptions;
pub use local_persist_stage::LocalPersistStage;
pub use local_relative_path::LocalRelativePath;
pub use local_root::LocalRoot;
pub use local_root_atomic_writer::LocalRootAtomicWriter;
pub use local_temp_dir::LocalTempDir;
pub use local_temp_file::LocalTempFile;
