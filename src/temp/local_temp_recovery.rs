// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::path::PathBuf;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FileDispositionInfo;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

use super::internal::NativePrivateEntry;
use super::internal::RecoveryResource;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
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
    /// Diagnostic path associated with the retained resource.
    path: PathBuf,
    /// Identity observed when ownership was established, if available.
    identity: Option<LocalFileIdentity>,
    /// Cleanup authority retained until cleanup succeeds or ownership
    /// transfers.
    resource: Option<RecoveryResource>,
}

impl LocalTempRecovery {
    /// Transfers a live temporary file's cleanup owner into recovery.
    ///
    /// The path remains diagnostic; cleanup uses the file owner's retained
    /// authority and identity. The returned recovery value retries cleanup on
    /// explicit calls and makes one best-effort attempt when dropped.
    ///
    /// # Parameters
    ///
    /// * `resource` — live temporary-file owner whose cleanup responsibility is
    ///   transferred.
    ///
    /// # Returns
    ///
    /// A recovery owner retaining the file and its current identity.
    pub(crate) fn from_file(resource: super::LocalTempFile) -> Self {
        let path = resource.path().to_path_buf();
        let identity = resource.current_identity();
        Self {
            path,
            identity,
            resource: Some(RecoveryResource::LiveFile(Box::new(resource))),
        }
    }

    /// Creates recovery ownership for one native entry.
    ///
    /// `path` is stored for diagnostics and error context. Deletion still
    /// requires the retained parent/name, open object, and matching identity.
    ///
    /// # Parameters
    ///
    /// * `path` — diagnostic path associated with the native entry.
    /// * `entry` — retained native handles, name, and observed identity.
    ///
    /// # Returns
    ///
    /// A recovery owner for the native entry.
    pub(crate) fn from_native(path: PathBuf, entry: NativePrivateEntry) -> Self {
        let entry_path = path.clone();
        Self {
            path,
            identity: entry.identity.clone(),
            resource: Some(RecoveryResource::Native(vec![(entry_path, entry)])),
        }
    }

    /// Creates recovery ownership for entries reported under one diagnostic
    /// path.
    ///
    /// Entry order is cleanup order. Successfully removed entries are released
    /// even if a later entry fails; the owner then retains only the remainder.
    ///
    /// # Parameters
    ///
    /// * `path` — diagnostic path associated with every entry.
    /// * `entries` — native entries in cleanup order.
    ///
    /// # Returns
    ///
    /// A recovery owner retaining the entries in cleanup order.
    pub(crate) fn from_native_entries(path: PathBuf, entries: Vec<NativePrivateEntry>) -> Self {
        let identity = entries.first().and_then(|entry| entry.identity.clone());
        let entries = entries.into_iter().map(|entry| (path.clone(), entry)).collect();
        Self {
            path,
            identity,
            resource: Some(RecoveryResource::Native(entries)),
        }
    }

    /// Creates recovery ownership while retaining a diagnostic path per entry.
    ///
    /// The entry paths do not grant access; each native entry's retained parent
    /// handle and identity determine whether cleanup may remove it.
    ///
    /// # Parameters
    ///
    /// * `path` — primary diagnostic path for this recovery owner.
    /// * `entries` — diagnostic path and retained native entry pairs in cleanup
    ///   order.
    ///
    /// # Returns
    ///
    /// A recovery owner retaining each entry and its diagnostic path.
    pub(crate) fn from_native_entries_with_paths(path: PathBuf, entries: Vec<(PathBuf, NativePrivateEntry)>) -> Self {
        let identity = entries.first().and_then(|(_, entry)| entry.identity.clone());
        Self {
            path,
            identity,
            resource: Some(RecoveryResource::Native(entries)),
        }
    }

    /// Returns the diagnostic namespace path.
    ///
    /// The borrowed path is context for diagnostics only and does not keep the
    /// retained resource alive or grant authority to remove it.
    #[must_use]
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the native identity when it was established, or `None` when
    /// cleanup cannot verify the object identity and therefore refuses to
    /// delete the entry by path alone.
    ///
    /// # Returns
    ///
    /// The observed identity, or `None` when identity verification is
    /// unavailable.
    #[inline]
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }

    /// Returns the live file resource, if this owner contains one.
    ///
    /// Taking the resource transfers cleanup responsibility to the caller;
    /// native-entry recovery cannot be converted into a `LocalTempFile`.
    ///
    /// # Returns
    ///
    /// The live file owner when present; native-entry recovery returns `None`.
    pub(crate) fn into_file(mut self) -> Option<super::LocalTempFile> {
        match self.resource.take()? {
            RecoveryResource::LiveFile(file) => Some(*file),
            RecoveryResource::Native(_) => None,
        }
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
    ///
    /// # Parameters
    ///
    /// * `self` — recovery owner whose retained resources are cleaned in order.
    ///
    /// # Returns
    ///
    /// `Ok(())` when all retained resources are released; otherwise returns the
    /// first cleanup error and retains resources that were not removed.
    pub fn cleanup(&mut self) -> crate::LocalResult<()> {
        let Some(resource) = self.resource.as_mut() else {
            return Ok(());
        };
        let result = match resource {
            RecoveryResource::LiveFile(resource) => resource.cleanup(),
            RecoveryResource::Native(entries) => {
                let mut completed = 0;
                let mut failure = None;
                for (path, entry) in entries.iter_mut() {
                    match cleanup_native_entry(entry, path).map_err(|error| {
                        error
                            .with_operation(crate::LocalFileOperation::Cleanup)
                            .with_path(path.to_path_buf())
                    }) {
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

    /// Combines two native recovery owners, preserving their cleanup order.
    ///
    /// Returns both original owners unchanged if either contains a live-file
    /// resource, since only native entry lists can be combined.
    ///
    /// # Parameters
    ///
    /// * `self` — first native owner, whose entries remain before the other's.
    /// * `other` — second owner to append when it also contains native entries.
    ///
    /// # Returns
    ///
    /// `Ok` contains the combined owner. `Err` returns both owners unchanged if
    /// either contains a live-file resource.
    ///
    /// # Errors
    ///
    /// Returns both owners when either contains a live-file resource, because
    /// only native-entry resources can be combined.
    pub(crate) fn append_native_owner(mut self, mut other: Self) -> Result<Self, (Self, Self)> {
        let Some(RecoveryResource::Native(mut other_entries)) = other.resource.take() else {
            return Err((self, other));
        };
        let Some(RecoveryResource::Native(entries)) = self.resource.as_mut() else {
            other.resource = Some(RecoveryResource::Native(other_entries));
            return Err((self, other));
        };
        entries.append(&mut other_entries);
        Ok(self)
    }
}

/// Validates a retained native entry before its name can be removed.
///
/// # Parameters
///
/// * `entry` — retained native authority and expected identity to validate.
/// * `path` — diagnostic context attached to observation errors.
///
/// # Returns
///
/// The retained file and expected identity after the opened object still
/// matches the identity and object kind recorded for the entry.
///
/// # Errors
///
/// Returns an error if the retained file or identity is missing, observation
/// fails, or the observed identity or object kind no longer matches.
fn validate_native_entry<'a>(
    entry: &'a NativePrivateEntry,
    path: &Path,
) -> crate::LocalResult<(&'a std::fs::File, &'a LocalFileIdentity)> {
    let Some(file) = entry.file.as_ref() else {
        return Err(
            LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup)
                .with_path(path.to_path_buf()),
        );
    };
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
    Ok((file, expected))
}

/// Removes one validated entry through its retained native authority.
///
/// Unix uses parent-relative `fstatat` and `unlinkat`, so owner-read
/// permission is not an extra prerequisite. `path` supplies diagnostic
/// context if the platform removal operation fails.
///
/// # Parameters
///
/// * `entry` — retained parent handle, name, and expected object kind.
/// * `path` — diagnostic context attached to removal errors.
/// * `file` — retained handle whose identity was validated.
/// * `expected` — identity validated against the retained handle.
///
/// # Returns
///
/// `Ok(())` when the entry is safely removed.
///
/// # Errors
///
/// Returns an error when the platform removal operation fails.
fn remove_native_entry(
    entry: &NativePrivateEntry,
    path: &Path,
    file: &std::fs::File,
    expected: &LocalFileIdentity,
) -> crate::LocalResult<()> {
    #[cfg(unix)]
    let _ = file;
    #[cfg(unix)]
    {
        let name = std::ffi::CString::new(entry.name.as_bytes())
            .map_err(|_| LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::Cleanup))?;
        let mut named_metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                entry.parent.as_raw_fd(),
                name.as_ptr(),
                named_metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(path.to_path_buf()),
                None,
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: fstatat succeeded and initialized the metadata value.
        let named_metadata = unsafe { named_metadata.assume_init() };
        // `dev_t` and `ino_t` are not u64 on every Unix target.
        #[allow(clippy::unnecessary_cast)]
        let named_identity = LocalFileIdentity::unix(named_metadata.st_dev as u64, named_metadata.st_ino as u64);
        let named_type = u64::from(named_metadata.st_mode) & u64::from(libc::S_IFMT);
        let expected_type = if entry.directory {
            u64::from(libc::S_IFDIR)
        } else {
            u64::from(libc::S_IFREG)
        };
        if &named_identity != expected || named_type != expected_type {
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
        let _ = (entry, expected);
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
        let _ = (entry, file, expected, path);
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            LocalFileOperation::Cleanup,
        ));
    }
    Ok(())
}

/// Removes one retained native entry after checking its identity and type.
///
/// Validation is kept separate from platform deletion so a name is never
/// removed solely because it occupies the expected path.
///
/// # Parameters
///
/// * `entry` — retained native authority and expected identity to validate and
///   release.
/// * `path` — diagnostic context attached to cleanup errors.
///
/// # Returns
///
/// `Ok(())` when the entry is safely removed; otherwise returns the cleanup
/// failure.
///
/// # Errors
///
/// Returns an error when identity validation or the platform removal fails.
fn cleanup_native_entry(entry: &mut NativePrivateEntry, path: &Path) -> crate::LocalResult<()> {
    let (file, expected) = validate_native_entry(entry, path)?;
    remove_native_entry(entry, path, file, expected)?;
    entry.file.take();
    Ok(())
}

impl Drop for LocalTempRecovery {
    /// Makes one best-effort cleanup attempt when the owner is dropped.
    ///
    /// Any cleanup error is intentionally discarded because `Drop` cannot
    /// return it; callers that need to inspect or retry failures should call
    /// [`LocalTempRecovery::cleanup`] before dropping the value.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
