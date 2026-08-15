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
mod local_read_options;
mod local_rename_options;
mod local_temp_directory_options;
mod local_temp_file_options;
mod local_walk_error_policy;
mod local_write_mode;
mod local_write_options;

pub use local_copy_options::LocalCopyOptions;
pub use local_copy_source_mode::LocalCopySourceMode;
pub use local_create_directory_options::LocalCreateDirectoryOptions;
pub use local_delete_options::LocalDeleteOptions;
pub use local_directory_reopen_policy::LocalDirectoryReopenPolicy;
pub use local_list_options::LocalListOptions;
pub use local_metadata_preserve_policy::LocalMetadataPreservePolicy;
pub use local_read_options::LocalReadOptions;
pub use local_rename_options::LocalRenameOptions;
pub use local_temp_directory_options::LocalTempDirectoryOptions;
pub use local_temp_file_options::LocalTempFileOptions;
pub use local_walk_error_policy::LocalWalkErrorPolicy;
pub use local_write_mode::LocalWriteMode;
pub use local_write_options::LocalWriteOptions;
