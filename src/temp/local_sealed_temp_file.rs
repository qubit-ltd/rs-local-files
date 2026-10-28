// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use super::LocalSealedTempFile;
use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::observation::LocalObservationRequirement;
use crate::temp::internal::LocalTempResourceBackend;

impl LocalSealedTempFile {
    /// Returns the namespace path of the sealed file.
    pub fn path(&self) -> &Path {
        self.resource.path()
    }

    /// Returns a native path for host resources.
    ///
    /// Returns `Some` for Host authority and `None` for Rooted authority, whose
    /// namespace paths are not native paths that callers may use independently.
    pub fn native_path(&self) -> Option<&Path> {
        matches!(self.resource.core.backend, LocalTempResourceBackend::Host(_)).then(|| self.path())
    }

    /// Observes the retained read-only handle.
    ///
    /// Returns `Err` after cleanup has closed the retained handle, or when the
    /// required native observation cannot be obtained.
    pub fn observation(&self) -> LocalResult<crate::observation::LocalFileObservation> {
        let file = self.resource.file.as_ref().ok_or_else(|| {
            LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(self.path().to_path_buf()),
                None,
                super::closed_file_error(),
            )
        })?;
        crate::observation::query::query_observation(file, LocalObservationRequirement::Required)
    }

    /// Removes the sealed file and its sandbox.
    ///
    /// On success, the retained observation handle is closed; subsequent calls
    /// to [`Self::observation`] return an error.
    pub fn cleanup(&mut self) -> LocalResult<()> {
        self.resource.cleanup()
    }
}
