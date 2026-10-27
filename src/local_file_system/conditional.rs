// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use digest::Digest;
use digest::Output;

use super::LocalFileError;
use super::LocalFileOperation;
use super::LocalFileSystem;
use super::LocalResult;
use super::LocalWriteOptions;
use super::Path;
use crate::capability::LocalConditionalKind;
use crate::local::conditional::ConditionalWriterBackend;
use crate::local_file_system_validation::validate_conditional_open;
use crate::options::LocalWritePrecondition;
use crate::writer::LocalConditionalWriter;

impl LocalFileSystem {
    /// Opens a writer that publishes only when the supplied destination
    /// condition holds.
    ///
    /// Enable the optional `digest` feature, which uses `digest` 0.11, and
    /// select an algorithm crate such as `sha2` 0.11. `Content` compares the
    /// full output of `D` over the current target and
    /// requires `CreateOrReplace`; `Absent` requests create-only publication.
    /// `Append` and symlink-following policies are rejected. The operation is
    /// not a strict compare-and-swap: the digest checks surround a native
    /// publication protocol and cannot prevent writes through an already-open
    /// handle.
    ///
    /// Mode, symlink-policy, scope, and capability checks run before path
    /// resolution or staging creation. `InvalidOptions` identifies invalid
    /// option combinations, `Unsupported` identifies unavailable backend
    /// support, and `RequirementNotMet` identifies an unavailable requested
    /// guarantee. Path and native failures are reported only after those
    /// static checks pass.
    ///
    /// Backend support differs by platform. Linux and macOS support Host and
    /// Rooted `Content`; on Windows, Host supports `Content` with
    /// `PreserveExisting` metadata only, and Rooted does not support `Content`.
    /// These unsupported Windows combinations return `Unsupported` while
    /// opening the writer, before resolving the path or creating staging
    /// resources. `Absent` is available only where the backend has a native
    /// no-overwrite install. Android and FreeBSD support is not claimed.
    ///
    /// The returned writer owns staged B until
    /// [`LocalConditionalWriter::commit`] consumes it. A commit error owns
    /// the retained recovery files; use its
    /// [`candidate`](crate::error::LocalConditionalCommitError::candidate)
    /// reader or transfer the files with
    /// [`into_recovery_files`](crate::error::LocalConditionalCommitError::into_recovery_files).
    /// Dropping a recovery handle does not delete its file.
    ///
    /// # Errors
    ///
    /// Returns an error when the mode, symlink policy, path, or selected
    /// backend cannot support the requested conditional write.
    pub fn open_conditional_writer<D: Digest>(
        &self,
        path: &Path,
        precondition: LocalWritePrecondition<Output<D>>,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalConditionalWriter<D>> {
        let kind = match &precondition {
            LocalWritePrecondition::Absent => LocalConditionalKind::Absent,
            LocalWritePrecondition::Content(_) => LocalConditionalKind::Content,
        };
        validate_conditional_open(self.scope(), self.core.capabilities, self.symlink_policy, kind, options).map_err(
            |error| {
                let error = error.with_path(path.to_path_buf());
                if matches!(
                    error.kind(),
                    crate::LocalFileErrorKind::RequirementNotMet | crate::LocalFileErrorKind::Unsupported
                ) {
                    super::with_current_directory(error, self.current_directory.virtual_path())
                } else {
                    error
                }
            },
        )?;
        let resolver = self.resolver_for(path, LocalFileOperation::OpenWriter)?;
        let resolved = super::resolve_operation_path(&resolver, path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
        super::reject_directory_qualified_file(
            &resolved,
            LocalFileOperation::OpenWriter,
            resolver.current_directory(),
        )?;
        let backend = match &self.core.namespace {
            crate::local::LocalNamespace::Host => {
                let host_path =
                    crate::local::resolve_host_path(resolved.authority_relative(), self.symlink_policy, false)
                        .map_err(|error| {
                            super::operation_error(
                                error,
                                LocalFileOperation::OpenWriter,
                                resolved.namespace_absolute(),
                                None,
                                resolver.current_directory(),
                            )
                        })?;
                ConditionalWriterBackend::host(&host_path, options).map_err(|error| {
                    super::with_current_directory(
                        LocalFileError::from_io(
                            LocalFileOperation::OpenWriter,
                            Some(resolved.namespace_absolute().to_path_buf()),
                            None,
                            error,
                        ),
                        resolver.current_directory(),
                    )
                })?
            }
            crate::local::LocalNamespace::Rooted(rooted) => rooted
                .open_conditional_backend(
                    resolved.authority_relative(),
                    resolved.namespace_absolute(),
                    None,
                    options,
                )
                .map_err(|error| match resolver.current_directory() {
                    Some(directory) => error.with_current_directory(directory.to_path_buf()),
                    None => error,
                })?,
        };
        #[cfg(all(feature = "digest", feature = "test-support"))]
        let mut backend = backend;
        #[cfg(all(feature = "digest", feature = "test-support"))]
        if let Some(hooks) = &self.core.conditional_hooks {
            backend.set_hooks(hooks.clone());
        }
        Ok(LocalConditionalWriter::new(
            resolved.namespace_absolute().to_path_buf(),
            if matches!(&self.core.namespace, crate::local::LocalNamespace::Host) {
                Some(resolved.authority_relative().to_path_buf())
            } else {
                None
            },
            resolver.current_directory().map(Path::to_path_buf),
            backend,
            precondition,
            *options,
        ))
    }
}
