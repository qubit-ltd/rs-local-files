// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use super::LocalTempFile;
use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::observation::LocalObservationRequirement;
use crate::temp::internal::LocalTempResourceBackend;

/// A cleanup-owned temporary file with no public write handle.
///
/// Sealing removes this library owner's write access, but does not guarantee
/// immutability against other processes or aliases. [`Self::observation`]
/// reports the retained read-only handle; it is an observation, not a content
/// version. [`Self::native_path`] is available only for Host authority and is
/// provided for observation, not as a way to bypass the bound authority.
/// After [`Self::cleanup`] closes the retained handle, `observation` returns an
/// error.
///
/// # Examples
///
/// ```no_run
/// use qubit_io::Output;
/// use qubit_local_files::{LocalFileSystem, LocalSealOptions};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut file = filesystem.create_temp_file()?;
/// file.write_fully(b"sealed contents")?;
/// let mut sealed = file.seal_with_options(&LocalSealOptions::default())?;
/// let current = sealed.observation()?;
/// assert_eq!(current.metadata().len(), 15);
/// sealed.cleanup()?;
/// # Ok(())
/// # }
/// ```
///
/// The sealed owner does not implement [`qubit_io::ByteOutput`]:
///
/// ```compile_fail
/// fn requires_output<T: qubit_io::ByteOutput>() {}
/// requires_output::<qubit_local_files::LocalSealedTempFile>();
/// ```
///
/// It also exposes no mutable native file handle:
///
/// ```compile_fail
/// fn request_mutable_file(file: &mut qubit_local_files::LocalSealedTempFile) {
///     let _ = file.as_file_mut();
/// }
/// ```
#[must_use = "use explicit cleanup to observe errors; drop only attempts cleanup"]
#[derive(Debug)]
pub struct LocalSealedTempFile {
    /// The original resource guard retained with its read-only file handle.
    resource: LocalTempFile,
}

impl LocalSealedTempFile {
    /// Wraps a resource after sealing has removed its write handle.
    ///
    /// # Parameters
    ///
    /// `resource` is the guard retaining cleanup authority and the read-only
    /// handle.
    ///
    /// # Returns
    ///
    /// A sealed owner retaining the supplied resource.
    pub(super) fn new(resource: LocalTempFile) -> Self {
        Self { resource }
    }

    /// Returns the namespace path of the sealed file.
    ///
    /// # Returns
    ///
    /// The path in the creating filesystem's namespace.
    pub fn path(&self) -> &Path {
        self.resource.path()
    }

    /// Returns a native path for host resources.
    ///
    /// Returns `Some` for Host authority and `None` for Rooted authority, whose
    /// namespace paths are not native paths that callers may use independently.
    ///
    /// # Returns
    ///
    /// `Some(path)` for Host authority, or `None` for Rooted authority.
    pub fn native_path(&self) -> Option<&Path> {
        matches!(self.resource.core.backend, LocalTempResourceBackend::Host(_)).then(|| self.path())
    }

    /// Observes the retained read-only handle.
    ///
    /// Returns `Err` after cleanup has closed the retained handle, or when the
    /// required native observation cannot be obtained.
    ///
    /// # Returns
    ///
    /// The required observation of the retained read-only handle.
    ///
    /// # Errors
    ///
    /// Returns an error after cleanup closes the handle or native observation
    /// fails.
    pub fn observation(&self) -> LocalResult<crate::observation::LocalFileObservation> {
        let file = self.resource.file.as_ref().ok_or_else(|| {
            LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(self.path().to_path_buf()),
                None,
                super::closed_file_error(),
            )
        })?;
        crate::observation::query::query_observation(file, LocalObservationRequirement::Required).map_err(|error| {
            let error = if error.path().is_none() {
                error.with_path(self.path().to_path_buf())
            } else {
                error
            };
            self.resource.core.contextualize_error(error)
        })
    }

    /// Removes the sealed file and its sandbox.
    ///
    /// On success, the retained observation handle is closed; subsequent calls
    /// to [`Self::observation`] return an error.
    ///
    /// # Returns
    ///
    /// `Ok(())` after cleanup completes.
    ///
    /// # Errors
    ///
    /// Returns a cleanup error when identity inspection or removal fails.
    pub fn cleanup(&mut self) -> LocalResult<()> {
        self.resource.cleanup()
    }
}
