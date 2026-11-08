// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt::Debug;
use std::io;

use crate::LocalFileReader;
use crate::LocalResult;

/// Read-only permission to reopen and validate one retained native entry.
///
/// Implementations use retained namespace authority and validate the current
/// entry identity. A diagnostic path is not an access capability. Owning this
/// permission allows future reads but does not schedule deletion of the entry.
/// Reopening must fail when the current identity differs from the retained one.
pub(crate) trait RecoveryAccess: Debug + Send + Sync {
    /// Reopens the retained entry and verifies that its identity is unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be opened or its identity changed.
    fn open_reader(&self) -> LocalResult<LocalFileReader>;

    /// Synchronizes the verified entry through its retained native authority.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if identity validation or synchronization fails.
    fn sync_file(&self) -> io::Result<()>;
}
