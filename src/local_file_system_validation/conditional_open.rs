// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystemCapabilities;
use crate::LocalFileSystemScope;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalWriteOptions;
use crate::capability::LocalConditionalKind;
use crate::local_file_system_validation::validate_write_options;

/// Validates conditional writer configuration before resolving its path.
pub(crate) fn validate_conditional_open(
    scope: LocalFileSystemScope,
    capabilities: LocalFileSystemCapabilities,
    symlink_policy: LocalSymlinkPolicy,
    kind: LocalConditionalKind,
    options: &LocalWriteOptions,
) -> LocalResult<()> {
    #[cfg(not(windows))]
    let _ = scope;
    let operation = LocalFileOperation::OpenWriter;
    if options.mode() == crate::LocalWriteMode::Append {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("conditional writes do not support append mode"));
    }
    if kind == LocalConditionalKind::Content && options.mode() == crate::LocalWriteMode::CreateNew {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("Content precondition requires CreateOrReplace mode"));
    }
    if symlink_policy.follows() {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("conditional writes require a non-following symlink policy"));
    }
    #[cfg(windows)]
    if scope == LocalFileSystemScope::Rooted
        && options.durability() == crate::policy::LocalDurabilityRequirement::Required
    {
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, operation)
            .with_reason("Windows Rooted conditional writes do not support required namespace durability"));
    }
    validate_write_options(options, capabilities, operation)?;
    #[cfg(windows)]
    if kind == LocalConditionalKind::Content
        && (scope == LocalFileSystemScope::Rooted
            || options.metadata_policy() != crate::options::LocalWriteMetadataPolicy::PreserveExisting)
    {
        return Err(LocalFileError::new(LocalFileErrorKind::Unsupported, operation)
            .with_reason("this Windows conditional Content configuration is unsupported"));
    }
    Ok(())
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
