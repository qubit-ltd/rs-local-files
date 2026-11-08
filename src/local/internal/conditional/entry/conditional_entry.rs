// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::super::RecoveryAccess;
use crate::LocalFileReader;
use crate::LocalResult;
use crate::recovery::LocalRecoveryFile;
use crate::recovery::LocalRecoveryFileRole;

/// An observed namespace entry and the permission required to read it again.
///
/// Its role changes only when the protocol verifies a state transition. Moving
/// the access object into recovery transfers the read permission and paths
/// without scheduling deletion. `namespace_path` identifies the retained
/// namespace entry, while `diagnostic_path` is optional user-facing context and
/// grants no authority to access it. The role is protocol state, not an
/// automatic cleanup policy.
#[derive(Debug)]
pub(crate) struct ConditionalEntry {
    /// Protocol role assigned after the corresponding state is verified.
    role: LocalRecoveryFileRole,
    /// Namespace location used by the retained access permission.
    namespace_path: PathBuf,
    /// Optional display context that grants no access authority.
    diagnostic_path: Option<PathBuf>,
    /// Permission to reopen, validate, and synchronize this entry.
    access: Box<dyn RecoveryAccess>,
}

impl ConditionalEntry {
    /// Creates an entry retaining its namespace description and read
    /// permission.
    ///
    /// # Parameters
    ///
    /// - `role`: Protocol state already established for the observed entry.
    /// - `namespace_path`: Namespace location associated with retained access.
    /// - `diagnostic_path`: Optional display context; it grants no authority.
    /// - `access`: Permission that validates the entry identity when reopened.
    ///
    /// # Returns
    ///
    /// An entry that owns the supplied paths and read permission.
    pub(crate) fn new(
        role: LocalRecoveryFileRole,
        namespace_path: PathBuf,
        diagnostic_path: Option<PathBuf>,
        access: Box<dyn RecoveryAccess>,
    ) -> Self {
        Self {
            role,
            namespace_path,
            diagnostic_path,
            access,
        }
    }

    /// Returns the entry's current role in the conditional write attempt.
    ///
    /// # Returns
    ///
    /// The role established by the conditional-write protocol.
    #[inline]
    pub(crate) const fn role(&self) -> LocalRecoveryFileRole {
        self.role
    }

    /// Returns the namespace path used to identify the retained entry.
    ///
    /// # Returns
    ///
    /// The namespace path borrowed for the duration of this entry borrow.
    #[inline]
    pub(crate) fn namespace_path(&self) -> &Path {
        &self.namespace_path
    }

    /// Changes the role after the corresponding verification or native action.
    ///
    /// Callers must update the role only after the operation that establishes
    /// that role has succeeded; in particular, a created but unverified copy
    /// remains `StagedCandidate`.
    ///
    /// # Parameters
    ///
    /// - `role`: The protocol role established by the completed operation.
    #[inline]
    pub(crate) fn set_role(&mut self, role: LocalRecoveryFileRole) {
        self.role = role;
    }

    /// Reopens the entry through its retained permission and validates
    /// identity.
    ///
    /// # Errors
    ///
    /// Returns the access implementation's error if reopening fails or the
    /// current entry identity does not match the retained identity.
    #[allow(dead_code)]
    #[inline]
    pub(crate) fn open_reader(&self) -> LocalResult<LocalFileReader> {
        self.access.open_reader()
    }

    /// Opens the entry through its retained authority, verifies the observed
    /// identity on that handle, and synchronizes the same handle.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if opening, identity validation, or synchronization
    /// fails.
    #[inline]
    pub(crate) fn sync_file(&self) -> io::Result<()> {
        self.access.sync_file()
    }

    /// Transfers the retained permission into a non-deleting recovery value.
    ///
    /// The supplied role must describe the verified state at transfer time.
    ///
    /// # Parameters
    ///
    /// - `role`: The verified recovery role to expose to the recovery owner.
    ///
    /// # Returns
    ///
    /// A recovery value that retains access and path metadata without
    /// scheduling deletion.
    pub(crate) fn into_recovery(self, role: LocalRecoveryFileRole) -> LocalRecoveryFile {
        LocalRecoveryFile::new(role, self.namespace_path, self.diagnostic_path, self.access)
    }
}
