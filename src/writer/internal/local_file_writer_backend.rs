// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use std::fs::File;

use super::LocalStagedCommitError;
use crate::local::LocalAtomicWriteError;
use crate::local::LocalAtomicWriter;
use crate::local::LocalRootAtomicWriter;

/// Native backend selected for one writer session.
#[derive(Debug)]
pub(crate) enum LocalFileWriterBackend {
    /// Same-directory staged publication.
    Staged(LocalAtomicWriter),
    /// Descriptor- or handle-relative same-directory staged publication.
    Rooted(LocalRootAtomicWriter),
    /// Direct append to an existing file.
    Append(File),
}

impl LocalFileWriterBackend {
    /// Commits a staged backend and preserves it when retry remains safe.
    ///
    /// # Panics
    ///
    /// Panics when called for the direct append backend.
    pub(crate) fn commit_staged(self) -> Result<bool, LocalStagedCommitError> {
        match self {
            Self::Staged(writer) => writer
                .commit_recoverable_with_durability()
                .map_err(|commit_error| {
                    let (error, retained) = commit_error.into_parts();
                    LocalStagedCommitError {
                        error,
                        backend: retained.map(Self::Staged).map(Box::new),
                    }
                }),
            Self::Rooted(writer) => writer
                .commit_recoverable_with_durability()
                .map_err(|commit_error| {
                    let (error, retained) = commit_error.into_parts();
                    LocalStagedCommitError {
                        error,
                        backend: retained.map(Self::Rooted).map(Box::new),
                    }
                }),
            Self::Append(_) => {
                unreachable!("direct append does not support staged commit")
            }
        }
    }

    /// Aborts a staged backend and removes its temporary file.
    ///
    /// # Panics
    ///
    /// Panics when called for the direct append backend.
    pub(crate) fn abort_staged(&mut self) -> Result<(), LocalAtomicWriteError> {
        match self {
            Self::Staged(writer) => writer.abort(),
            Self::Rooted(writer) => writer.abort(),
            Self::Append(_) => {
                unreachable!("direct append does not support staged abort")
            }
        }
    }
}
