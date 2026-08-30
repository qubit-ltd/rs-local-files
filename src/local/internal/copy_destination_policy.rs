// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared copy destination policy decisions.
// qubit-style: allow source-test-pair
// qubit-style: allow explicit-imports

use super::CopyDestinationAction;
use crate::LocalCopyConflictPolicy;
use crate::LocalCopyTypeConflictPolicy;

/// Selects the destination action without performing filesystem I/O.
///
/// # Parameters
///
/// - `source_is_directory`: Whether the source entry is a directory.
/// - `destination_is_directory`: `None` for an absent destination, otherwise
///   the observed destination kind.
/// - `conflict`: Policy for two non-directory entries.
/// - `type_conflict`: Policy when only one entry is a directory.
///
/// # Returns
///
/// `Some` with the permitted action, or `None` when the selected policy
/// requires a conflict failure.
#[must_use]
pub(crate) const fn decide_copy_destination(
    source_is_directory: bool,
    destination_is_directory: Option<bool>,
    conflict: LocalCopyConflictPolicy,
    type_conflict: LocalCopyTypeConflictPolicy,
) -> Option<CopyDestinationAction> {
    let Some(destination_is_directory) = destination_is_directory else {
        return Some(CopyDestinationAction::Create);
    };
    if source_is_directory && destination_is_directory {
        return Some(CopyDestinationAction::Merge);
    }
    if source_is_directory != destination_is_directory {
        return match type_conflict {
            LocalCopyTypeConflictPolicy::Fail => None,
            LocalCopyTypeConflictPolicy::Replace => Some(CopyDestinationAction::Replace),
            LocalCopyTypeConflictPolicy::Skip => Some(CopyDestinationAction::Skip),
        };
    }
    match conflict {
        LocalCopyConflictPolicy::Fail => None,
        LocalCopyConflictPolicy::Overwrite => Some(CopyDestinationAction::Replace),
        LocalCopyConflictPolicy::Skip => Some(CopyDestinationAction::Skip),
    }
}
