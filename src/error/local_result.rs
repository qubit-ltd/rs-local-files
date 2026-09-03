// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalFileError;

/// Result type used by local filesystem operations.
///
/// # Examples
///
/// ```
/// use qubit_local_files::error::LocalResult;
///
/// fn validate() -> LocalResult<()> {
///     Ok(())
/// }
///
/// assert!(validate().is_ok());
/// ```
pub type LocalResult<T> = Result<T, LocalFileError>;
