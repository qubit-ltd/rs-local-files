// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::LocalFileError;
use crate::LocalTempAccessState;
use crate::LocalTempFile;

/// Failure while sealing, retaining the temporary resource for recovery.
///
/// The original [`LocalFileError`] is available through [`Self::cause`] and
/// the standard error source chain. The retained temporary file remains owned
/// by this error until it is extracted with [`Self::into_resource`] or the
/// error is dropped.
#[derive(Debug)]
pub struct LocalTempSealError {
    cause: LocalFileError,
    access_state: LocalTempAccessState,
}

impl LocalTempSealError {
    pub(crate) const fn new(cause: LocalFileError, access_state: LocalTempAccessState) -> Self {
        Self { cause, access_state }
    }

    /// Returns the sealing failure.
    ///
    /// This is a borrow; inspecting the cause does not consume the retained
    /// temporary file.
    pub fn cause(&self) -> &LocalFileError {
        &self.cause
    }

    /// Returns the resource owner for recovery or cleanup.
    ///
    /// This consumes the error and transfers its retained file owner.
    pub fn into_resource(self) -> LocalTempFile {
        let (_, recovery) = self.cause.into_temp_recovery_parts();
        recovery
            .and_then(crate::temp::types::LocalTempRecovery::into_file)
            .expect("seal failure retains its temporary file")
    }

    /// Returns the access state established when sealing failed.
    pub const fn access_state(&self) -> LocalTempAccessState {
        self.access_state
    }
}

impl std::fmt::Display for LocalTempSealError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "temporary file sealing failed: {}", self.cause)
    }
}

impl std::error::Error for LocalTempSealError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
