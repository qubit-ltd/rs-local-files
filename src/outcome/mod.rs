// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured outcomes from native namespace operations.

mod internal;
mod local_copy_failure;
mod local_copy_failure_state;
mod local_copy_method;
mod local_copy_outcome;
mod local_copy_stats;
mod local_create_directory_outcome;
mod local_delete_outcome;
mod local_persist_cleanup_state;
mod local_persist_method;
mod local_persist_outcome;
mod local_rename_failure;
mod local_rename_failure_state;
mod local_rename_outcome;
mod local_write_publication_method;

pub use local_copy_failure::LocalCopyFailure;
pub use local_copy_failure::LocalCopyResult;
#[cfg(all(test, windows))]
pub(crate) use local_copy_failure::copy_failure_state;
pub use local_copy_failure_state::LocalCopyFailureState;
pub use local_copy_method::LocalCopyMethod;
pub use local_copy_outcome::LocalCopyOutcome;
pub use local_copy_stats::LocalCopyStats;
pub use local_create_directory_outcome::LocalCreateDirectoryOutcome;
pub use local_delete_outcome::LocalDeleteOutcome;
pub use local_persist_cleanup_state::LocalPersistCleanupState;
pub use local_persist_method::LocalPersistMethod;
pub use local_persist_outcome::LocalPersistOutcome;
pub use local_rename_failure::LocalRenameFailure;
pub use local_rename_failure::LocalRenameResult;
pub use local_rename_failure_state::LocalRenameFailureState;
pub use local_rename_outcome::LocalRenameOutcome;
pub use local_write_publication_method::LocalWritePublicationMethod;

pub use crate::local::LocalPersistFailureState;
pub use crate::local::LocalPersistStage;
pub use crate::local_file_kind::LocalFileKind;
pub use crate::local_file_metadata::LocalFileMetadata;
pub use crate::local_file_permissions::LocalFilePermissions;
pub use crate::walk::LocalDirectoryEntry;
pub use crate::writer::LocalWriteFailureState;
pub use crate::writer::LocalWriteOutcome;
pub use crate::writer::LocalWriterState;
