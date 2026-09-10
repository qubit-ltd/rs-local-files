// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Current authority retained by a temporary-resource handle.

/// Source authority independent of any particular publication attempt.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::outcome::LocalTempSourceState;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut resource = LocalFileSystem::host()?.create_temp_file()?;
/// assert_eq!(resource.source_state(), LocalTempSourceState::Owned);
/// resource.cleanup()?;
/// assert_eq!(resource.source_state(), LocalTempSourceState::Released);
/// resource.cleanup()?;
/// # Ok(())
/// # }
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTempSourceState {
    /// The original entity remains eligible for operations, subject to identity
    /// verification. This does not guarantee intact contents after partial
    /// cleanup.
    Owned,
    /// The entity left its source location; only private sandbox cleanup
    /// remains. Further publication is forbidden.
    CleanupRequired,
    /// No cleanup responsibility remains. Cleanup succeeds idempotently.
    Released,
    /// Source authority cannot be established. Publication and deletion,
    /// including automatic Drop cleanup, are forbidden.
    Indeterminate,
}
