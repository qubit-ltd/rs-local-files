// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! Concrete local filesystem APIs and their private implementation.

mod internal;
#[cfg(coverage)]
pub(crate) use internal::coverage_fault::is_enabled as coverage_fault_enabled;
#[cfg(coverage)]
pub(crate) use internal::coverage_fault::take_on_nth as take_coverage_fault_on_nth;
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
mod local_persist_error;
mod local_persist_options;
mod local_persist_stage;
mod local_relative_path;
mod local_root_atomic_writer;

#[cfg(unix)]
pub(crate) use internal::sync_rooted_parent;
pub(crate) use internal::{
    clean_dir_path, copy_dir_all_with_paths, copy_file_with_options,
    create_temp_dir_in_dir_with_affixes, create_temp_file_in_dir, dir_size_path, ensure_dir_path,
    ensure_parent_path, file_name_from_path, file_name_from_url, move_directory_without_replacing,
    move_file_without_replacing, normalize_extension, open_native_reader_path,
    open_native_writer_path, remove_any_path, replace_file, try_random_file_name,
    validate_portable_file_name_impl,
};
#[cfg(any(unix, windows))]
pub(crate) use internal::{
    create_rooted_directory, open_root_directory, open_rooted_native_reader,
    open_rooted_native_writer, read_root_directory, read_rooted_directory,
    read_rooted_symlink_metadata, remove_rooted_entry, rename_rooted_entry, set_rooted_permissions,
};

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
pub use local_persist_error::LocalPersistError;
pub use local_persist_options::LocalPersistOptions;
pub use local_persist_stage::LocalPersistStage;
pub use local_relative_path::LocalRelativePath;
pub use local_root_atomic_writer::LocalRootAtomicWriter;
