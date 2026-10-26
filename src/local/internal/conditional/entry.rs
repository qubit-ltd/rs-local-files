// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt::Debug;
use std::path::PathBuf;

use crate::LocalFileReader;
use crate::LocalResult;
use crate::recovery::LocalRecoveryFile;
use crate::recovery::LocalRecoveryFileRole;

/// Read-only permission to reopen and validate one retained native entry.
///
/// Implementations must use their retained namespace authority and validate
/// the current entry identity. A diagnostic path is not an access capability.
pub(crate) trait RecoveryAccess: Debug + Send + Sync {
    fn open_reader(&self) -> LocalResult<LocalFileReader>;
}

/// An observed namespace entry and the permission required to read it again.
///
/// Its role changes only when the protocol verifies a state transition. Moving
/// the access object into recovery transfers the read permission and paths
/// without scheduling deletion.
#[derive(Debug)]
pub(crate) struct ConditionalEntry {
    role: LocalRecoveryFileRole,
    namespace_path: PathBuf,
    diagnostic_path: Option<PathBuf>,
    access: Box<dyn RecoveryAccess>,
}

impl ConditionalEntry {
    /// Creates an entry retaining its namespace description and read
    /// permission.
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
    pub(crate) const fn role(&self) -> LocalRecoveryFileRole {
        self.role
    }

    /// Returns the namespace path used to identify the retained entry.
    pub(crate) fn namespace_path(&self) -> &std::path::Path {
        &self.namespace_path
    }

    /// Changes the role after the corresponding verification or native action.
    ///
    /// Callers must update the role only after the operation that establishes
    /// that role has succeeded; in particular, a created but unverified copy
    /// remains `StagedCandidate`.
    pub(crate) fn set_role(&mut self, role: LocalRecoveryFileRole) {
        self.role = role;
    }

    /// Reopens the entry through its retained permission and validates
    /// identity.
    #[allow(dead_code)]
    pub(crate) fn open_reader(&self) -> LocalResult<LocalFileReader> {
        self.access.open_reader()
    }

    /// Transfers the retained permission into a non-deleting recovery value.
    ///
    /// The supplied role must describe the verified state at transfer time.
    pub(crate) fn into_recovery(self, role: LocalRecoveryFileRole) -> LocalRecoveryFile {
        LocalRecoveryFile::new(role, self.namespace_path, self.diagnostic_path, self.access)
    }
}
