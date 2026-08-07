// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Concrete local filesystem APIs and their private implementation.

mod internal;
pub(crate) use internal::test_support::fault_error as test_fault_error;
pub(crate) use internal::test_support::io_error as test_io_error;
pub(crate) use internal::test_support::is_enabled as test_support_enabled;
#[cfg(feature = "internal-test-support")]
pub(crate) use internal::test_support::take as take_test_support;
#[cfg(feature = "internal-test-support")]
pub(crate) use internal::test_support::take_on_nth as take_test_support_on_nth;
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
mod local_persist_failure_state;
mod local_persist_options;
mod local_persist_stage;
mod local_relative_path;
mod local_root_atomic_writer;

#[cfg(unix)]
pub(crate) use internal::sync_rooted_parent;
pub(crate) use internal::{
    CopyDestinationAction,
    HostLocalFileSystem,
    LocalNamespace,
    copy_dir_all_with_paths,
    copy_dir_all_with_paths_scoped,
    copy_directory_guarantee_unavailable,
    copy_failure_published,
    copy_failure_unchanged,
    copy_file_replace_requires_atomicity,
    copy_file_with_options,
    copy_source_mode_mismatch,
    create_temp_dir_in_dir_with_affixes,
    create_temp_file_in_dir,
    decide_copy_destination,
    ensure_parent_path,
    ensure_parent_path_with_sync_dirs,
    ensure_required_directory_durability,
    internal_copy_options,
    move_directory_without_replacing,
    move_file_without_replacing,
    open_native_reader_path,
    open_native_writer_path,
    published_durability,
    rename_failure_after_native_attempt,
    rename_failure_renamed,
    rename_failure_unchanged,
    replace_file,
    resolve_host_path,
    root_authority_path,
    try_random_file_name,
    validate_portable_file_name_impl,
    validate_temp_affixes,
};
#[cfg(any(unix, windows))]
pub(crate) use internal::{
    RootedDirectoryReader,
    open_root_directory_reader,
    open_rooted_directory_reader,
};
#[cfg(any(unix, windows))]
pub(crate) use internal::{
    create_rooted_directory,
    open_root_directory,
    open_rooted_native_reader,
    open_rooted_native_writer,
    read_root_directory,
    read_rooted_directory,
    read_rooted_symlink_metadata,
    remove_rooted_entry,
    rename_rooted_entry,
    set_rooted_permissions,
};

pub(crate) use local_atomic_commit_error::LocalAtomicCommitError;
pub(crate) use local_atomic_destination_state::LocalAtomicDestinationState;
pub(crate) use local_atomic_write_error::LocalAtomicWriteError;
pub(crate) use local_atomic_write_options::LocalAtomicWriteOptions;
pub(crate) use local_atomic_write_stage::LocalAtomicWriteStage;
pub(crate) use local_atomic_writer::LocalAtomicWriter;
pub use local_copy_conflict_policy::LocalCopyConflictPolicy;
pub(crate) use local_copy_dir_error::LocalCopyDirError;
pub(crate) use local_copy_dir_options::LocalCopyDirOptions;
pub(crate) use local_copy_dir_stage::LocalCopyDirStage;
pub(crate) use local_copy_dir_stats::LocalCopyDirStats;
pub use local_copy_type_conflict_policy::LocalCopyTypeConflictPolicy;
pub use local_persist_error::LocalPersistError;
pub use local_persist_failure_state::LocalPersistFailureState;
pub use local_persist_options::LocalPersistOptions;
pub use local_persist_stage::LocalPersistStage;
pub use local_relative_path::LocalRelativePath;
pub(crate) use local_root_atomic_writer::LocalRootAtomicWriter;
