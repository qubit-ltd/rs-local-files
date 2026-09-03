// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation support for local filesystem operations.

mod active_fault;
mod atomic_commit_state;
mod atomic_file_install;
mod atomic_install_recovery;
#[cfg(unix)]
mod atomic_metadata;
#[cfg(unix)]
mod atomic_namespace_race;
mod atomic_staging_state;
mod copy_budget;
mod copy_destination_action;
mod copy_destination_policy;
mod copy_dir;
mod copy_policy;
mod directory_identity;
mod file_io;
mod file_move;
mod file_name_generation;
mod file_name_validation;
mod host_local_file_system;
mod io_result_context;
mod local_atomic_publication_mode;
mod local_namespace;
#[cfg(unix)]
mod opened_atomic_destination;
mod operation_policy;
#[cfg(windows)]
mod owned_unicode_string;
mod path_io_error;
mod path_operations;
mod publication_state;
#[cfg(unix)]
mod rooted_atomic_install;
#[cfg(unix)]
mod rooted_atomic_namespace_race;
#[cfg(unix)]
mod rooted_atomic_write;
#[cfg(any(unix, windows))]
mod rooted_directory_reader;
#[cfg(unix)]
mod rooted_file_io;
#[cfg(unix)]
mod rooted_io_result;
#[cfg(unix)]
mod rooted_namespace_io;
#[cfg(unix)]
mod rooted_parent;
#[cfg(unix)]
mod rooted_parent_mode;
#[cfg(unix)]
mod rooted_staged_file;
#[cfg(unix)]
mod rooted_staging_retry;
mod rooted_symlink_create_error;
mod rooted_symlink_create_failure_state;
mod staged_file;
mod temp_entry;
pub(crate) mod test_fault_guard;
pub(crate) mod test_support;
#[cfg(unix)]
mod unix_nonblocking;
#[cfg(unix)]
mod unix_stat;
#[cfg(windows)]
mod windows_rooted;
#[cfg(windows)]
mod windows_rooted_staged_file;

pub(crate) use atomic_commit_state::commit_recoverably;
pub(crate) use atomic_commit_state::finalize_failed_commit;
pub(crate) use atomic_commit_state::synchronize_staging_file;
pub(crate) use atomic_file_install::install_atomic_file;
pub(crate) use atomic_install_recovery::AtomicInstallRecovery;
pub(crate) use atomic_install_recovery::recover_atomic_install_error;
#[cfg(unix)]
pub(crate) use atomic_metadata::preserve_atomic_metadata;
#[cfg(unix)]
pub(crate) use atomic_namespace_race::verify_atomic_destination_identity;
pub(crate) use atomic_staging_state::AtomicStagingState;
pub use copy_budget::CopyBudget;
pub use copy_destination_action::CopyDestinationAction;
pub use copy_destination_policy::decide_copy_destination;
pub(crate) use copy_dir::copy_dir_all_with_paths;
pub(crate) use copy_dir::copy_dir_all_with_paths_scoped;
pub(crate) use copy_dir::copy_file_with_options;
pub(crate) use copy_policy::copy_directory_guarantee_unavailable;
pub(crate) use copy_policy::copy_file_replace_requires_atomicity;
pub(crate) use copy_policy::copy_source_mode_mismatch;
pub(crate) use directory_identity::DirectoryIdentity;
pub(crate) use file_io::open_native_reader_path;
pub(crate) use file_io::open_native_writer_path;
pub(crate) use file_move::move_directory_without_replacing;
pub(crate) use file_move::move_file_without_replacing;
pub(crate) use file_move::parent_dir_for;
pub(crate) use file_move::replace_file;
pub(crate) use file_move::sync_parent_dir;
pub(crate) use file_name_generation::try_random_file_name;
pub(crate) use host_local_file_system::HostLocalFileSystem;
pub(crate) use host_local_file_system::internal_copy_options;
pub(crate) use host_local_file_system::resolve_host_path;
pub use local_atomic_publication_mode::LocalAtomicPublicationMode;
pub(crate) use local_namespace::LocalNamespace;
#[cfg(unix)]
pub(crate) use opened_atomic_destination::OpenedAtomicDestination;
#[cfg(unix)]
pub(crate) use opened_atomic_destination::open_atomic_destination;
#[cfg(unix)]
pub(super) use opened_atomic_destination::open_rooted_atomic_destination;
pub(crate) use operation_policy::ensure_required_directory_durability;
#[cfg(windows)]
pub(super) use owned_unicode_string::OwnedUnicodeString;
pub(crate) use path_operations::absolute_path;
pub(crate) use path_operations::add_path_context;
pub(crate) use path_operations::ensure_parent_path_with_sync_dirs;
pub(crate) use publication_state::copy_failure_indeterminate;
pub(crate) use publication_state::copy_failure_published;
pub(crate) use publication_state::copy_failure_unchanged;
pub(crate) use publication_state::published_durability;
pub(crate) use publication_state::rename_failure_after_native_attempt;
pub(crate) use publication_state::rename_failure_renamed;
pub(crate) use publication_state::rename_failure_unchanged;
#[cfg(unix)]
pub(super) use rooted_atomic_install::install_rooted_atomic_file;
#[cfg(unix)]
pub(super) use rooted_atomic_namespace_race::verify_rooted_atomic_destination_identity;
#[cfg(unix)]
pub(super) use rooted_atomic_write::create_rooted_staged_file;
#[cfg(unix)]
pub(super) use rooted_atomic_write::inspect_rooted_atomic_destination;
#[cfg(any(unix, windows))]
pub(crate) use rooted_directory_reader::RootedDirectoryReader;
#[cfg(unix)]
pub(crate) use rooted_file_io::open_root_directory;
#[cfg(unix)]
pub(crate) use rooted_file_io::open_rooted_native_reader;
#[cfg(unix)]
pub(crate) use rooted_file_io::open_rooted_native_writer;
#[cfg(unix)]
pub(super) use rooted_file_io::open_rooted_parent;
#[cfg(unix)]
pub(crate) use rooted_file_io::read_rooted_symlink_metadata;
#[cfg(unix)]
pub(crate) use rooted_file_io::sync_rooted_parent;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::create_rooted_directory;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::create_rooted_symlink;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::open_root_directory_reader;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::open_rooted_directory_reader;
#[cfg(unix)]
#[cfg(unix)]
pub(crate) use rooted_namespace_io::read_rooted_directory;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::read_rooted_link;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::remove_rooted_entry;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::rename_rooted_entry;
#[cfg(unix)]
pub(crate) use rooted_namespace_io::set_rooted_permissions;
#[cfg(unix)]
pub(super) use rooted_parent_mode::RootedParentMode;
#[cfg(unix)]
pub(super) use rooted_staged_file::RootedStagedFile;
pub(crate) use rooted_symlink_create_error::RootedSymlinkCreateError;
pub(crate) use rooted_symlink_create_failure_state::RootedSymlinkCreateFailureState;
pub(crate) use staged_file::StagedFile;
pub(crate) use temp_entry::create_temp_dir_in_dir_with_affixes;
pub(crate) use temp_entry::create_temp_file_in_dir;
pub(crate) use temp_entry::validate_temp_affixes;
#[cfg(unix)]
pub(crate) use unix_nonblocking::clear_nonblocking;
#[cfg(unix)]
pub(crate) use unix_nonblocking::open_with_nonblocking_retry;
#[cfg(windows)]
pub(crate) use windows_rooted::create_rooted_directory;
#[cfg(windows)]
pub(crate) use windows_rooted::create_rooted_symlink;
#[cfg(windows)]
pub(crate) use windows_rooted::open_root_directory;
#[cfg(windows)]
pub(crate) use windows_rooted::open_root_directory_reader;
#[cfg(windows)]
pub(crate) use windows_rooted::open_rooted_directory_reader;
#[cfg(windows)]
pub(crate) use windows_rooted::open_rooted_native_reader;
#[cfg(windows)]
pub(crate) use windows_rooted::open_rooted_native_writer;
#[cfg(windows)]
pub(crate) use windows_rooted::probe_windows_limits;
#[cfg(windows)]
pub(crate) use windows_rooted::probe_windows_space;
#[cfg(windows)]
pub(crate) use windows_rooted::read_rooted_directory;
#[cfg(windows)]
pub(crate) use windows_rooted::read_rooted_link;
#[cfg(windows)]
pub(crate) use windows_rooted::read_rooted_symlink_metadata;
#[cfg(windows)]
pub(crate) use windows_rooted::remove_rooted_entry;
#[cfg(windows)]
pub(crate) use windows_rooted::rename_rooted_entry;
#[cfg(windows)]
pub(crate) use windows_rooted::rooted_link_targets_directory;
#[cfg(windows)]
pub(crate) use windows_rooted::set_rooted_permissions;
#[cfg(windows)]
pub(super) use windows_rooted_staged_file::WindowsRootedStagedFile;
