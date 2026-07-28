// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured outcomes from native namespace operations.

mod local_copy_method;
mod local_copy_outcome;
mod local_copy_stats;
mod local_create_directory_outcome;
mod local_delete_outcome;
mod local_rename_outcome;

pub use local_copy_method::LocalCopyMethod;
pub use local_copy_outcome::LocalCopyOutcome;
pub use local_copy_stats::LocalCopyStats;
pub use local_create_directory_outcome::LocalCreateDirectoryOutcome;
pub use local_delete_outcome::LocalDeleteOutcome;
pub use local_rename_outcome::LocalRenameOutcome;
