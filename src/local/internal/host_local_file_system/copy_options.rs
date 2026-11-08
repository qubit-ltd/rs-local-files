// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Instant;

use super::LocalCopyOptions;
use super::LocalMetadataPreservePolicy;
use super::LocalSymlinkPolicy;

/// Converts unified copy policy to the shared native implementation.
///
/// # Parameters
/// - `options`: Unified public copy options.
/// - `symlink_policy`: Owning filesystem policy used without an override.
/// - `started_at`: Original operation start, including preflight time, used for
///   the tree-copy deadline.
///
/// # Returns
/// Tree traversal, conflict, metadata, and durability options. The facade
/// handles source mode, atomicity, and top-level parent creation separately.
pub(crate) fn internal_copy_options(
    options: &LocalCopyOptions,
    symlink_policy: LocalSymlinkPolicy,
    started_at: Instant,
) -> crate::local::LocalCopyDirOptions {
    let symlink_policy = options.symlink_policy_override().unwrap_or(symlink_policy);
    let mut result = crate::local::LocalCopyDirOptions::new()
        .with_conflict(options.conflict())
        .with_type_conflict(options.type_conflict())
        .with_symlink_policy(symlink_policy)
        .with_durability(options.durability())
        .with_started_at(started_at);
    if let Some(value) = options.max_depth() {
        result = result.with_max_depth(value);
    }
    if let Some(value) = options.max_entries() {
        result = result.with_max_entries(value);
    }
    if let Some(value) = options.max_bytes() {
        result = result.with_max_bytes(value);
    }
    if let Some(value) = options.max_open_directories() {
        result = result.with_max_open_directories(value);
    }
    if let Some(value) = options.deadline() {
        result = result.with_deadline(value);
    }
    if options.preserve_metadata() == LocalMetadataPreservePolicy::Permissions {
        result = result.preserve_permissions();
    }
    result
}
