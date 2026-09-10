// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Named persistence error fields retaining both recovery axes.

use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalPersistFailureState;
use crate::LocalPersistStage;
use crate::outcome::LocalTempSourceState;

/// Consumed persistence error and its retained temporary resource.
///
/// `state` describes this call's publication; `source_state` is the source
/// snapshot at failure. Query the resource for its current state after
/// mutation.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::error::LocalPersistErrorParts;
/// use qubit_local_files::options::LocalTempFileOptions;
/// use qubit_local_files::outcome::LocalPersistFailureState;
/// use qubit_local_files::outcome::LocalTempSourceState;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let parent = tempfile::tempdir()?;
/// let target = parent.path().join("existing");
/// std::fs::write(&target, b"preserved")?;
/// let resource = LocalFileSystem::host()?.create_temp_file_with_options(
///     &LocalTempFileOptions::new().with_parent(parent.path()),
/// )?;
/// let error = resource.persist(&target).expect_err("target already exists");
/// let LocalPersistErrorParts { mut resource, state, source_state, .. } = error.into_parts();
/// assert_eq!(state, LocalPersistFailureState::NotPublished);
/// assert_eq!(source_state, LocalTempSourceState::Owned);
/// resource.cleanup()?;
/// assert_eq!(std::fs::read(&target)?, b"preserved");
/// # Ok(())
/// # }
/// ```
#[must_use = "the parts retain a temporary resource and recovery facts"]
#[derive(Debug)]
pub struct LocalPersistErrorParts<T> {
    /// Structured failure with native cause and path context.
    pub error: LocalFileError,
    /// Resource retained for operations allowed by its current source state.
    pub resource: T,
    /// Target supplied to this call.
    pub requested_target: PathBuf,
    /// Bound target, when resolution completed.
    pub resolved_target: Option<PathBuf>,
    /// Stage at which this call failed.
    pub stage: LocalPersistStage,
    /// Publication fact for this call only.
    pub state: LocalPersistFailureState,
    /// Source authority snapshot captured when this error was constructed.
    pub source_state: LocalTempSourceState,
}
