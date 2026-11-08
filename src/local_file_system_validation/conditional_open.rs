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
use crate::local_file_system_validation::conditional_protocol_compiled;
use crate::local_file_system_validation::validate_write_options;

/// Validates conditional writer configuration before resolving its path.
/// This helper performs no path resolution or filesystem I/O.
///
/// # Parameters
///
/// - `scope`: Namespace scope used to select the platform publication protocol.
/// - `capabilities`: Capabilities available to the filesystem instance.
/// - `symlink_policy`: Policy must reject symlink following for conditional
///   writes.
/// - `kind`: Conditional publication precondition; `Content` cannot use
///   `CreateNew` mode.
/// - `options`: Writer mode and guarantees; `Append` is unsupported, and
///   Windows Rooted writes cannot require namespace durability.
///
/// # Returns
///
/// Returns `Ok(())` when the supplied configuration is valid and its
/// conditional publication protocol is compiled for this scope and target.
///
/// # Errors
///
/// Returns `InvalidOptions` for unsupported mode or symlink combinations,
/// `RequirementNotMet` for unavailable required Windows Rooted durability,
/// or propagates write-option validation errors. Returns `Unsupported` when
/// the requested publication protocol is unavailable for this scope and target.
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
    if !conditional_protocol_compiled(scope, kind, options) {
        return Err(LocalFileError::new(LocalFileErrorKind::Unsupported, operation)
            .with_reason("this conditional publication protocol is unsupported on this platform"));
    }
    Ok(())
}
