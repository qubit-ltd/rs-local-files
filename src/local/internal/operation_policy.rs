// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared preflight policy for host and rooted operations.

use std::path::Path;

use crate::LocalDurabilityRequirement;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Checks whether an operation can provide its required directory-durability
/// guarantee before it mutates the filesystem namespace.
///
/// # Parameters
///
/// - `requirement` selects whether directory durability is optional, preferred,
///   or required.
/// - `operation` identifies the operation reported if the guarantee is
///   unavailable.
/// - `source` and `target` provide the paths attached to that failure.
/// - `supported` reports whether the operation can synchronize the affected
///   directory.
/// - `reason` explains why the guarantee is unavailable.
///
/// # Returns
///
/// Returns `Ok(())` unless durability is required and unsupported.
///
/// # Errors
///
/// Returns `LocalFileErrorKind::RequirementNotMet` with the operation, reason,
/// source path, and target path when durability is required but unsupported.
pub(crate) fn ensure_required_directory_durability(
    requirement: LocalDurabilityRequirement,
    operation: LocalFileOperation,
    source: &Path,
    target: &Path,
    supported: bool,
    reason: &'static str,
) -> LocalResult<()> {
    if requirement == LocalDurabilityRequirement::Required && !supported {
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, operation)
            .with_reason(reason)
            .with_path(source.to_path_buf())
            .with_target(target.to_path_buf()));
    }
    Ok(())
}
