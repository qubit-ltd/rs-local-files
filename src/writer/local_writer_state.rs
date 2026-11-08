// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by writer integration tests.

/// Lifecycle state of a native writer publication session.
///
/// # Examples
///
/// ```
/// use qubit_io::Output;
/// use tempfile::tempdir;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::outcome::LocalWriterState;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let directory = tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let writer = filesystem.open_writer(&directory.path().join("output.txt"))?;
/// assert_eq!(writer.state(), LocalWriterState::Open);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalWriterState {
    /// The writer has not completed a terminal operation.
    ///
    /// Stream operations are available only while no failure state is
    /// recorded; after staged abort cleanup fails, only abort may be retried.
    Open,
    /// Commit completed successfully.
    Committed,
    /// Abort completed without publishing staged content.
    Aborted,
}
