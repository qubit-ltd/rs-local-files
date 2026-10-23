// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::{
    LocalFileError, LocalFileErrorKind, LocalFileOperation, LocalFileSystem, LocalResult,
    LocalWriteOptions, Path,
};
use crate::local::conditional::ConditionalWriterBackend;
use crate::options::LocalWritePrecondition;
use crate::writer::LocalConditionalWriter;
use digest::{Digest, Output};

impl LocalFileSystem {
    /// Opens a writer that publishes only when the supplied destination condition holds.
    pub fn open_conditional_writer<D: Digest>(
        &self,
        path: &Path,
        precondition: LocalWritePrecondition<Output<D>>,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalConditionalWriter<D>> {
        if options.mode() == crate::LocalWriteMode::Append {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::OpenWriter,
            )
            .with_reason("conditional writes do not support append mode")
            .with_path(path.to_path_buf()));
        }
        if matches!(&precondition, LocalWritePrecondition::Content(_))
            && options.mode() == crate::LocalWriteMode::CreateNew
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::OpenWriter,
            )
            .with_reason("Content precondition requires CreateOrReplace mode")
            .with_path(path.to_path_buf()));
        }
        if self.symlink_policy.follows() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::OpenWriter,
            )
            .with_reason("conditional writes require a non-following symlink policy")
            .with_path(path.to_path_buf()));
        }
        let resolver = self.resolver_for(path, LocalFileOperation::OpenWriter)?;
        let resolved =
            super::resolve_operation_path(&resolver, path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(
            &resolved,
            LocalFileOperation::OpenWriter,
            resolver.current_directory(),
        )?;
        super::reject_directory_qualified_file(
            &resolved,
            LocalFileOperation::OpenWriter,
            resolver.current_directory(),
        )?;
        let backend = match &self.core.namespace {
            crate::local::LocalNamespace::Host => {
                ConditionalWriterBackend::host(resolved.authority_relative(), options).map_err(
                    |error| {
                        super::with_current_directory(
                            LocalFileError::from_io(
                                LocalFileOperation::OpenWriter,
                                Some(resolved.namespace_absolute().to_path_buf()),
                                None,
                                error,
                            ),
                            resolver.current_directory(),
                        )
                    },
                )?
            }
            crate::local::LocalNamespace::Rooted(rooted) => {
                let authority = rooted.open_conditional_backend(
                    resolved.authority_relative(),
                    resolved.namespace_absolute(),
                    options,
                )?;
                return Ok(LocalConditionalWriter::new(
                    resolved.namespace_absolute().to_path_buf(),
                    None,
                    authority,
                    precondition,
                    *options,
                ));
            }
        };
        Ok(LocalConditionalWriter::new(
            resolved.namespace_absolute().to_path_buf(),
            Some(resolved.authority_relative().to_path_buf()),
            backend,
            precondition,
            *options,
        ))
    }
}
