// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(feature = "digest")]
use std::path::Path;

#[cfg(feature = "digest")]
use crate::LocalResult;
use crate::LocalWriteOptions;
use crate::capability::LocalCapabilitySupport;
use crate::capability::LocalConditionalKind;
use crate::directory::LocalDirectoryHandle;
#[cfg(feature = "digest")]
use crate::local_file_system_validation::validate_conditional_open;

impl LocalDirectoryHandle {
    /// Reports whether this fixed authority implements the requested
    /// conditional publication protocol. This query performs no filesystem
    /// mutation and does not guarantee runtime filesystem support.
    pub fn conditional_support(
        &self,
        kind: LocalConditionalKind,
        options: &LocalWriteOptions,
    ) -> LocalCapabilitySupport {
        if options.mode() == crate::LocalWriteMode::Append {
            return LocalCapabilitySupport::Unsupported;
        }
        if kind == LocalConditionalKind::Content && options.mode() == crate::LocalWriteMode::CreateNew {
            return LocalCapabilitySupport::Unsupported;
        }
        #[cfg(windows)]
        if kind == LocalConditionalKind::Content
            || options.durability() == crate::policy::LocalDurabilityRequirement::Required
        {
            return LocalCapabilitySupport::Unsupported;
        }
        #[cfg(unix)]
        let _ = kind;
        #[cfg(not(any(unix, windows)))]
        return LocalCapabilitySupport::Unsupported;
        LocalCapabilitySupport::CanAttempt
    }

    /// Opens a conditional writer beneath this fixed directory authority.
    ///
    /// On Windows, `Content` is unsupported for rooted authorities and is
    /// rejected before creating a candidate.
    /// Mode, symlink-policy, scope, and capability checks happen before
    /// resolving the relative path or creating staging resources. Errors
    /// preserve the distinction between `InvalidOptions`, `Unsupported`,
    /// `RequirementNotMet`, and later path or native I/O failures.
    #[cfg(feature = "digest")]
    pub fn open_conditional_writer<D: digest::Digest>(
        &self,
        relative: &Path,
        precondition: crate::options::LocalWritePrecondition<digest::Output<D>>,
        options: &LocalWriteOptions,
    ) -> LocalResult<crate::writer::LocalConditionalWriter<D>> {
        let kind = if matches!(&precondition, crate::options::LocalWritePrecondition::Absent) {
            LocalConditionalKind::Absent
        } else {
            LocalConditionalKind::Content
        };
        validate_conditional_open(
            self.filesystem().scope(),
            self.filesystem().capabilities(),
            self.filesystem().symlink_policy(),
            kind,
            options,
        )
        .map_err(|error| error.with_path(relative.to_path_buf()))?;
        self.filesystem()
            .open_conditional_writer(&self.checked(relative, true)?, precondition, options)
    }
}
