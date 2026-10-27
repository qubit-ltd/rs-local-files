// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Options shared by host-wide and rooted local filesystem operations.

mod local_copy_options;
mod local_copy_source_mode;
mod local_create_directory_options;
mod local_delete_options;
mod local_directory_reopen_policy;
mod local_list_options;
mod local_metadata_preserve_policy;
mod local_observed_read_options;
mod local_path_binding_options;
mod local_read_all_options;
mod local_read_options;
mod local_rename_options;
mod local_seal_options;
mod local_snapshot_options;
mod local_temp_cleanup_limits;
mod local_temp_directory_options;
mod local_temp_file_options;
mod local_walk_error_policy;
mod local_write_metadata_policy;
mod local_write_mode;
mod local_write_options;
#[cfg(feature = "digest")]
mod local_write_precondition;
mod resource_limits;

pub use local_copy_options::LocalCopyOptions;
pub use local_copy_source_mode::LocalCopySourceMode;
pub use local_create_directory_options::LocalCreateDirectoryOptions;
pub use local_delete_options::LocalDeleteOptions;
pub use local_directory_reopen_policy::LocalDirectoryReopenPolicy;
mod local_directory_open_options;
pub use local_directory_open_options::LocalDirectoryOpenOptions;
pub use local_list_options::LocalListOptions;
pub use local_metadata_preserve_policy::LocalMetadataPreservePolicy;
pub use local_observed_read_options::LocalObservedReadOptions;
pub use local_path_binding_options::LocalPathBindingOptions;
pub use local_read_all_options::LocalReadAllOptions;
pub use local_read_options::LocalReadOptions;
pub use local_rename_options::LocalRenameOptions;
pub use local_seal_options::LocalSealOptions;
pub use local_snapshot_options::LocalSnapshotOptions;
pub use local_snapshot_options::LocalSnapshotOptionsBuilder;
pub use local_temp_cleanup_limits::LocalTempCleanupLimits;
pub use local_temp_directory_options::LocalTempDirectoryOptions;
pub use local_temp_file_options::LocalTempFileOptions;
pub use local_walk_error_policy::LocalWalkErrorPolicy;
pub use local_write_metadata_policy::LocalWriteMetadataPolicy;
pub use local_write_mode::LocalWriteMode;
pub use local_write_options::LocalWriteOptions;
#[cfg(feature = "digest")]
pub use local_write_precondition::LocalWritePrecondition;

pub use crate::directory::LocalTreeValidationOptions;
pub use crate::local::LocalCopyConflictPolicy;
pub use crate::local::LocalCopyTypeConflictPolicy;
pub use crate::local::LocalPersistOptions;

mod local_tree_publish_options;
pub use local_tree_publish_options::LocalTreePublishOptions;
