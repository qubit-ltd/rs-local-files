// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal contracts used by the crate's integration-test suite.

pub use crate::local::CopyBudget;
pub use crate::local::CopyDestinationAction;
pub use crate::local::LocalAtomicCommitError;
pub use crate::local::LocalAtomicDestinationState;
pub use crate::local::LocalAtomicPublicationMode;
pub use crate::local::LocalAtomicWriteError;
pub use crate::local::LocalAtomicWriteOptions;
pub use crate::local::LocalAtomicWriteStage;
pub use crate::local::LocalCopyDirError;
pub use crate::local::LocalCopyDirOptions;
pub use crate::local::LocalCopyDirStage;
pub use crate::local::LocalCopyDirStats;
pub use crate::local::LocalRelativePath;
pub use crate::local::decide_copy_destination;
pub use crate::path::LocalNamespacePath;
pub use crate::path::LocalPathResolver;
pub use crate::read::OpenOptions as InternalReadOptions;
pub use crate::rooted::EntryKind;
pub use crate::rooted::Metadata;
pub use crate::rooted::Permissions;
pub use crate::rooted::Root;
pub use crate::write::Mode;
pub use crate::write::OpenOptions as InternalWriteOptions;

/// Drives copy accounting with a deterministic monotonic clock.
#[doc(hidden)]
pub fn copy_with_clock(
    budget: &mut CopyBudget,
    reader: &mut dyn std::io::Read,
    writer: &mut dyn std::io::Write,
    now: &mut dyn FnMut() -> std::time::Instant,
) -> std::io::Result<u64> {
    budget.copy_with_now(reader, writer, now)
}

/// Derives the generated publication target for a temporary resource path.
#[doc(hidden)]
pub fn generated_keep_target(resource: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    crate::temp::internal::generated_target(resource)
}

/// Maps internal copy-stage facts to the public recovery state.
#[cfg(windows)]
pub const fn copy_failure_state(
    stage: crate::local::LocalCopyDirStage,
    stats: crate::outcome::LocalCopyStats,
) -> crate::outcome::LocalCopyFailureState {
    crate::outcome::copy_failure_state(stage, stats)
}
