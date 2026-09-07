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

use crate::LocalAtomicityRequirement;
use crate::LocalCopySourceMode;
use crate::LocalCopyTypeConflictPolicy;
use crate::LocalDurabilityRequirement;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;

/// Validates the actual source entry before any destination mutation.
///
/// # Parameters
///
/// - `kind`: Metadata kind observed without following the final link.
/// - `mode`: Complete source interpretation selected for this operation.
///
/// # Errors
///
/// Returns `Unsupported` for special files in every mode, or
/// `RequirementNotMet` when an entry/tree requirement rejects a supported kind.
#[inline]
pub(crate) fn validate_copy_source_kind(
    kind: LocalFileKind,
    mode: LocalCopySourceMode,
) -> Result<(), LocalFileErrorKind> {
    match kind {
        LocalFileKind::File | LocalFileKind::Symlink if mode != LocalCopySourceMode::Tree => Ok(()),
        LocalFileKind::Directory if mode != LocalCopySourceMode::Entry => Ok(()),
        LocalFileKind::File | LocalFileKind::Symlink | LocalFileKind::Directory => {
            Err(LocalFileErrorKind::RequirementNotMet)
        }
        _ => Err(LocalFileErrorKind::Unsupported),
    }
}

/// Reports whether a non-regular source asks for an unsupported guarantee.
///
/// Directory trees cannot provide whole-operation atomicity or durability.
/// Symbolic-link copying cannot provide whole-operation atomicity. File and
/// link durability also depends on platform capabilities and synchronization.
#[inline]
pub(crate) fn copy_source_guarantee_unavailable(
    source_kind: LocalFileKind,
    atomicity: LocalAtomicityRequirement,
    durability: LocalDurabilityRequirement,
) -> bool {
    (source_kind != LocalFileKind::File && atomicity == LocalAtomicityRequirement::Required)
        || (source_kind == LocalFileKind::Directory && durability == LocalDurabilityRequirement::Required)
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
