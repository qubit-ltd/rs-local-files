// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error construction shared by directory mutations.

use std::io;
use std::path::Path;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;

/// Adds partial-publication state after a directory mutation has changed the
/// tree.
pub(crate) fn directory_mutation_error(
    operation: LocalFileOperation,
    path: &Path,
    changed: bool,
    source: io::Error,
) -> LocalFileError {
    let error = LocalFileError::from_io(operation, Some(path.to_path_buf()), None, source);
    if changed {
        error.with_kind(LocalFileErrorKind::PublicationIncomplete)
    } else {
        error
    }
}
