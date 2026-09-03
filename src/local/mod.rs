// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Concrete local filesystem APIs and their private implementation.

mod internal;
#[cfg(feature = "test-support")]
pub use internal::test_fault_guard::TestFaultGuard;
pub(crate) use internal::test_support::fault_error as test_fault_error;
#[cfg(feature = "test-support")]
pub use internal::test_support::install_test_fault;
pub(crate) use internal::test_support::io_error as test_io_error;
pub(crate) use internal::test_support::is_enabled as test_support_enabled;
#[cfg(feature = "test-support")]
pub(crate) use internal::test_support::take as take_test_support;
#[cfg(feature = "test-support")]
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
mod local_root_atomic_writer_support;

pub use internal::CopyBudget;
pub use internal::CopyDestinationAction;
pub(crate) use internal::DirectoryIdentity;
pub(crate) use internal::HostLocalFileSystem;
#[cfg(test)]
pub(crate) use internal::LocalAtomicPublicationMode;
pub(crate) use internal::LocalNamespace;
#[cfg(any(unix, windows))]
pub(crate) use internal::RootedDirectoryReader;
pub(crate) use internal::RootedSymlinkCreateError;
pub(crate) use internal::RootedSymlinkCreateFailureState;
pub(crate) use internal::copy_dir_all_with_paths;
pub(crate) use internal::copy_dir_all_with_paths_scoped;
pub(crate) use internal::copy_directory_guarantee_unavailable;
pub(crate) use internal::copy_failure_published;
pub(crate) use internal::copy_failure_unchanged;
pub(crate) use internal::copy_file_replace_requires_atomicity;
pub(crate) use internal::copy_file_with_options;
pub(crate) use internal::copy_source_mode_mismatch;
#[cfg(any(unix, windows))]
pub(crate) use internal::create_rooted_directory;
#[cfg(any(unix, windows))]
pub(crate) use internal::create_rooted_symlink;
pub(crate) use internal::create_temp_dir_in_dir_with_affixes;
pub(crate) use internal::create_temp_file_in_dir;
pub use internal::decide_copy_destination;
pub(crate) use internal::ensure_parent_path_with_sync_dirs;
pub(crate) use internal::ensure_required_directory_durability;
pub(crate) use internal::internal_copy_options;
pub(crate) use internal::move_directory_without_replacing;
pub(crate) use internal::move_file_without_replacing;
pub(crate) use internal::open_native_reader_path;
pub(crate) use internal::open_native_writer_path;
#[cfg(any(unix, windows))]
pub(crate) use internal::open_root_directory;
#[cfg(any(unix, windows))]
pub(crate) use internal::open_root_directory_reader;
#[cfg(any(unix, windows))]
pub(crate) use internal::open_rooted_directory_reader;
#[cfg(any(unix, windows))]
pub(crate) use internal::open_rooted_native_reader;
#[cfg(any(unix, windows))]
pub(crate) use internal::open_rooted_native_writer;
#[cfg(windows)]
pub(crate) use internal::probe_windows_limits;
#[cfg(windows)]
pub(crate) use internal::probe_windows_space;
pub(crate) use internal::published_durability;
#[cfg(any(unix, windows))]
#[cfg(any(unix, windows))]
pub(crate) use internal::read_rooted_directory;
#[cfg(any(unix, windows))]
pub(crate) use internal::read_rooted_link;
#[cfg(any(unix, windows))]
pub(crate) use internal::read_rooted_symlink_metadata;
#[cfg(any(unix, windows))]
pub(crate) use internal::remove_rooted_entry;
pub(crate) use internal::rename_failure_after_native_attempt;
pub(crate) use internal::rename_failure_renamed;
pub(crate) use internal::rename_failure_unchanged;
#[cfg(any(unix, windows))]
pub(crate) use internal::rename_rooted_entry;
pub(crate) use internal::replace_file;
pub(crate) use internal::resolve_host_path;
#[cfg(windows)]
pub(crate) use internal::rooted_link_targets_directory;
#[cfg(any(unix, windows))]
pub(crate) use internal::set_rooted_permissions;
#[cfg(unix)]
pub(crate) use internal::sync_rooted_parent;
pub(crate) use internal::try_random_file_name;
pub(crate) use internal::validate_temp_affixes;
pub use local_atomic_commit_error::LocalAtomicCommitError;
pub use local_atomic_destination_state::LocalAtomicDestinationState;
pub use local_atomic_write_error::LocalAtomicWriteError;
pub use local_atomic_write_options::LocalAtomicWriteOptions;
pub use local_atomic_write_stage::LocalAtomicWriteStage;
pub(crate) use local_atomic_writer::LocalAtomicWriter;
pub use local_copy_conflict_policy::LocalCopyConflictPolicy;
pub use local_copy_dir_error::LocalCopyDirError;
pub use local_copy_dir_options::LocalCopyDirOptions;
pub use local_copy_dir_stage::LocalCopyDirStage;
pub use local_copy_dir_stats::LocalCopyDirStats;
pub use local_copy_type_conflict_policy::LocalCopyTypeConflictPolicy;
pub use local_persist_error::LocalPersistError;
pub use local_persist_failure_state::LocalPersistFailureState;
pub use local_persist_options::LocalPersistOptions;
pub use local_persist_stage::LocalPersistStage;
pub use local_relative_path::LocalRelativePath;
pub(crate) use local_root_atomic_writer::LocalRootAtomicWriter;
