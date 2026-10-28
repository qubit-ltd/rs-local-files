// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(unix)]
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;

pub(crate) use super::internal::NativePrivateCreateError;
pub(crate) use super::internal::NativePrivateEntry;
use super::internal::RecoveryResource;
use crate::observation::LocalFileIdentity;

/// Owns a private temporary object left behind by a failed operation.
///
/// This value owns the cleanup responsibility. Dropping it makes one best-
/// effort cleanup attempt and silently discards any error. A path returned by
/// [`Self::path`] is only diagnostic information; keeping that path does not
/// keep this owner alive. Conditional-write recovery files retain access
/// without automatically deleting them; this type attempts to remove its
/// retained object when dropped.
///
/// An error from [`Self::cleanup`] may follow partial cleanup. Keep the owner
/// if you want to inspect the error or choose to retry; a later call retries
/// resources that remain owned. If the native identity is unavailable,
/// cleanup refuses to delete by path, since the path alone cannot establish
/// that the entry is still the retained object.
///
/// # Examples
///
/// The failed operation's error can own the recovery resource. Extract the
/// owner to keep that cleanup responsibility after handling the error:
///
/// ```
/// use qubit_local_files::LocalFileError;
/// use qubit_local_files::LocalTempRecovery;
///
/// fn take_recovery(error: LocalFileError) -> Option<LocalTempRecovery> {
///     let (_cause, recovery) = error.into_temp_recovery_parts();
///     recovery
/// }
///
/// fn try_cleanup(owner: &mut LocalTempRecovery) -> Result<(), LocalFileError> {
///     owner.cleanup()
/// }
/// ```
///
/// The returned owner still has drop-time cleanup responsibility. Keep it
/// alive while handling a cleanup error if you may retry:
///
/// ```
/// use qubit_local_files::LocalFileError;
/// use qubit_local_files::LocalTempRecovery;
///
/// fn cleanup_or_keep(owner: &mut LocalTempRecovery) -> Result<(), LocalFileError> {
///     match owner.cleanup() {
///         Ok(()) => Ok(()),
///         Err(error) => {
///             // The owner remains available for a later, caller-controlled retry.
///             Err(error)
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct LocalTempRecovery {
    path: PathBuf,
    identity: Option<LocalFileIdentity>,
    resource: Option<RecoveryResource>,
}

impl LocalTempRecovery {
    pub(crate) fn from_file(resource: super::LocalTempFile) -> Self {
        let path = resource.path().to_path_buf();
        let identity = resource.current_identity();
        Self {
            path,
            identity,
            resource: Some(RecoveryResource::LiveFile(Box::new(resource))),
        }
    }

    pub(crate) fn from_native(path: PathBuf, entry: NativePrivateEntry) -> Self {
        Self {
            path,
            identity: entry.identity.clone(),
            resource: Some(RecoveryResource::Native(vec![entry])),
        }
    }

    pub(crate) fn from_native_entries(path: PathBuf, entries: Vec<NativePrivateEntry>) -> Self {
        let identity = entries.first().and_then(|entry| entry.identity.clone());
        Self {
            path,
            identity,
            resource: Some(RecoveryResource::Native(entries)),
        }
    }

    pub(crate) fn into_file(mut self) -> Option<super::LocalTempFile> {
        match self.resource.take()? {
            RecoveryResource::LiveFile(file) => Some(*file),
            RecoveryResource::Native(_) => None,
        }
    }

    /// Returns the diagnostic namespace path.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Returns the native identity when it was established, or `None` when
    /// cleanup cannot verify the object identity and therefore refuses to
    /// delete the entry by path alone.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Removes retained objects only when their identities are known and
    /// still match.
    ///
    /// Successful entries are released as cleanup proceeds. If a later entry
    /// fails, earlier entries may already have been removed and the owner
    /// retains the resources that remain. Calling this method again retries
    /// those resources. When an identity is unavailable or no longer matches,
    /// cleanup refuses to delete the object by path.
    ///
    /// # Errors
    ///
    /// Returns a [`crate::LocalFileError`] if identity observation fails, the
    /// retained identity or object kind does not match, or the native removal
    /// operation fails. An error can therefore be returned after partial
    /// cleanup; keep this owner to retain responsibility for any remaining
    /// resources and decide whether to retry.
    pub fn cleanup(&mut self) -> crate::LocalResult<()> {
        let Some(resource) = self.resource.as_mut() else {
            return Ok(());
        };
        let result = match resource {
            RecoveryResource::LiveFile(resource) => resource.cleanup(),
            RecoveryResource::Native(entries) => {
                let mut completed = 0;
                let mut failure = None;
                for entry in entries.iter_mut() {
                    match cleanup_native_entry(entry, &self.path) {
                        Ok(()) => completed += 1,
                        Err(error) => {
                            failure = Some(error);
                            break;
                        }
                    }
                }
                if completed > 0 {
                    entries.drain(..completed);
                }
                failure.map_or(Ok(()), Err)
            }
        };
        if result.is_ok() {
            self.resource.take();
        }
        result
    }
}

fn cleanup_native_entry(entry: &mut NativePrivateEntry, path: &Path) -> crate::LocalResult<()> {
    use crate::LocalFileError;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;

    if entry.file.is_none() {
        return Err(
            LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup)
                .with_path(path.to_path_buf()),
        );
    }
    let file = entry.file.as_ref().expect("checked native recovery file");
    let Some(expected) = entry.identity.as_ref() else {
        return Err(
            LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup)
                .with_path(path.to_path_buf()),
        );
    };
    let observed =
        crate::observation::query::query_observation(file, crate::observation::LocalObservationRequirement::Required)?;
    if observed.identity() != Some(expected) {
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            LocalFileOperation::Cleanup,
        ));
    }
    if file.metadata().map(|metadata| metadata.is_dir()).ok() != Some(entry.directory) {
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            LocalFileOperation::Cleanup,
        ));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::fd::FromRawFd;
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(entry.name.as_bytes())
            .map_err(|_| LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::Cleanup))?;
        let named = unsafe {
            libc::openat(
                entry.parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if named < 0 {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(path.to_path_buf()),
                None,
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: openat returned a fresh descriptor owned here.
        let named_file = unsafe { File::from_raw_fd(named) };
        let named_identity = crate::observation::query::query_observation(
            &named_file,
            crate::observation::LocalObservationRequirement::Required,
        )?;
        if named_identity.identity() != Some(expected) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Cleanup,
            ));
        }
        let flags = if entry.directory { libc::AT_REMOVEDIR } else { 0 };
        if unsafe { libc::unlinkat(entry.parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(path.to_path_buf()),
                None,
                io::Error::last_os_error(),
            ));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;

        use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO;
        use windows_sys::Win32::Storage::FileSystem::FileDispositionInfo;
        use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        let removed = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&raw const disposition).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        if removed == 0 {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(path.to_path_buf()),
                None,
                io::Error::last_os_error(),
            ));
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, expected, path);
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            LocalFileOperation::Cleanup,
        ));
    }
    entry.file.take();
    Ok(())
}

impl Drop for LocalTempRecovery {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(all(test, unix))]
#[path = "local_temp_recovery/recovery_cleanup_tests.rs"]
mod recovery_cleanup_tests;
