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
use crate::local_file_system_validation::conditional_protocol_compiled;
#[cfg(feature = "digest")]
use crate::local_file_system_validation::validate_conditional_open;

impl LocalDirectoryHandle {
    /// Opens a conditional writer beneath this fixed directory authority.
    ///
    /// On Windows, `Content` is unsupported for rooted authorities and is
    /// rejected before creating a candidate.
    /// Mode, symlink-policy, scope, and capability checks happen before
    /// resolving the relative path or creating staging resources. Errors
    /// preserve the distinction between `InvalidOptions`, `Unsupported`,
    /// `RequirementNotMet`, and later path or native I/O failures.
    ///
    /// # Parameters
    /// - `relative`: Destination path resolved beneath this directory
    ///   authority.
    /// - `precondition`: Required absence or digest condition for publication.
    /// - `options`: Write mode and policy used to open the conditional writer.
    ///
    /// # Type Parameters
    /// - `D`: Digest algorithm used to check the content precondition.
    ///
    /// # Returns
    /// A writer that publishes only when the requested precondition still
    /// holds.
    ///
    /// # Errors
    /// Returns option, capability, requirement, path, resource, or native I/O
    /// errors; the specific kind distinguishes validation from later failures.
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

    /// Reports whether this fixed authority can attempt the requested
    /// conditional publication protocol. This query performs no filesystem
    /// mutation and does not guarantee runtime filesystem support.
    ///
    /// # Parameters
    /// - `kind`: Conditional publication protocol to query.
    /// - `options`: Write mode and policy whose compatibility is checked.
    ///
    /// # Returns
    /// `CanAttempt` when the compiled implementation supports an attempt, or
    /// `Unsupported` when the request is incompatible or unavailable.
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
        if !conditional_protocol_compiled(self.filesystem().scope(), kind, options) {
            return LocalCapabilitySupport::Unsupported;
        }
        LocalCapabilitySupport::CanAttempt
    }
}
