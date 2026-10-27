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
use crate::LocalFileError;
#[cfg(feature = "digest")]
use crate::LocalFileErrorKind;
#[cfg(feature = "digest")]
use crate::LocalFileOperation;
#[cfg(feature = "digest")]
use crate::LocalResult;
use crate::LocalWriteOptions;
use crate::capability::LocalCapabilitySupport;
use crate::capability::LocalConditionalKind;
use crate::directory::LocalDirectoryHandle;

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
        if self.conditional_support(kind, options) == LocalCapabilitySupport::Unsupported {
            return Err(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::OpenWriter)
                    .with_path(relative.to_path_buf())
                    .with_reason("the fixed-directory conditional protocol is unsupported"),
            );
        }
        self.filesystem()
            .open_conditional_writer(&self.checked(relative, true)?, precondition, options)
    }
}
