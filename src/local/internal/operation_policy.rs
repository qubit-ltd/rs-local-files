// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
// =============================================================================
//! Shared preflight policy for host and rooted operations.

use std::path::Path;

use crate::{
    LocalDurabilityRequirement,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalResult,
};

/// Rejects a required directory-durability guarantee before namespace
/// mutation.
pub(crate) fn ensure_required_directory_durability(
    requirement: LocalDurabilityRequirement,
    operation: LocalFileOperation,
    source: &Path,
    target: &Path,
    supported: bool,
    reason: &'static str,
) -> LocalResult<()> {
    if requirement == LocalDurabilityRequirement::Required && !supported {
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            operation,
        )
        .with_reason(reason)
        .with_path(source.to_path_buf())
        .with_target(target.to_path_buf()));
    }
    Ok(())
}
