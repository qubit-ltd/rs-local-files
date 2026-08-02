// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair -- exercised through copy integration
// tests.
//! Shared, side-effect-free copy policy decisions.

use crate::{
    LocalAtomicityRequirement,
    LocalCopySourceMode,
    LocalCopyTypeConflictPolicy,
    LocalDurabilityRequirement,
};

/// Reports whether the configured source mode rejects the observed kind.
#[inline]
pub(crate) fn copy_source_mode_mismatch(
    source_is_directory: bool,
    source_mode: LocalCopySourceMode,
) -> bool {
    matches!(
        (source_is_directory, source_mode),
        (true, LocalCopySourceMode::File) | (false, LocalCopySourceMode::Tree)
    )
}

/// Reports whether a directory copy asks for an unsupported guarantee.
#[inline]
pub(crate) fn copy_directory_guarantee_unavailable(
    source_is_directory: bool,
    atomicity: LocalAtomicityRequirement,
    durability: LocalDurabilityRequirement,
) -> bool {
    source_is_directory
        && (atomicity == LocalAtomicityRequirement::Required
            || durability == LocalDurabilityRequirement::Required)
}

/// Reports whether replacing a directory would violate required atomicity.
#[inline]
pub(crate) fn copy_file_replace_requires_atomicity(
    source_is_directory: bool,
    atomicity: LocalAtomicityRequirement,
    type_conflict: LocalCopyTypeConflictPolicy,
    target_is_directory: bool,
) -> bool {
    !source_is_directory
        && atomicity == LocalAtomicityRequirement::Required
        && type_conflict == LocalCopyTypeConflictPolicy::Replace
        && target_is_directory
}
