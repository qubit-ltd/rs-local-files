use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::observation::LocalFileIdentity;

/// Native private resource retained after a creation step partially succeeds.
#[derive(Debug)]
pub(crate) struct NativePrivateEntry {
    pub(crate) parent: File,
    pub(crate) name: OsString,
    pub(crate) file: Option<File>,
    pub(crate) identity: Option<LocalFileIdentity>,
    pub(crate) directory: bool,
}

/// Native private creation failure with ownership of any partial resource.
#[derive(Debug)]
pub(crate) struct NativePrivateCreateError {
    pub(crate) source: io::Error,
    pub(crate) retained: Option<NativePrivateEntry>,
}

/// Owns a private temporary object left behind by a failed operation.
#[derive(Debug)]
pub struct LocalTempRecovery {
    path: PathBuf,
    identity: Option<LocalFileIdentity>,
    resource: Option<RecoveryResource>,
}

#[derive(Debug)]
enum RecoveryResource {
    LiveFile(super::LocalTempFile),
    Native(Vec<NativePrivateEntry>),
}

impl LocalTempRecovery {
    pub(crate) fn from_file(resource: super::LocalTempFile) -> Self {
        let path = resource.path().to_path_buf();
        let identity = resource.current_identity();
        Self {
            path,
            identity,
            resource: Some(RecoveryResource::LiveFile(resource)),
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
        Self { path, identity, resource: Some(RecoveryResource::Native(entries)) }
    }

    pub(crate) fn into_file(mut self) -> Option<super::LocalTempFile> {
        match self.resource.take()? {
            RecoveryResource::LiveFile(file) => Some(file),
            RecoveryResource::Native(_) => None,
        }
    }

    /// Returns the diagnostic namespace path.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Returns the native identity when it was established.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Removes the retained object only when its identity is known and still matches.
    pub fn cleanup(&mut self) -> crate::LocalResult<()> {
        let Some(resource) = self.resource.as_mut() else { return Ok(()); };
        let result = match resource {
            RecoveryResource::LiveFile(resource) => resource.cleanup(),
            RecoveryResource::Native(entries) => {
                for entry in entries {
                    cleanup_native_entry(entry, &self.path)?;
                }
                Ok(())
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
        return Ok(());
    }
    let file = entry.file.as_ref().expect("checked native recovery file");
    let Some(expected) = entry.identity.as_ref() else {
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup));
    };
    let observed = crate::observation::query::query_observation(file, crate::observation::LocalObservationRequirement::Required)?;
    if observed.identity() != Some(expected) {
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup));
    }
    if file.metadata().map(|metadata| metadata.is_dir()).ok() != Some(entry.directory) {
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup));
    }
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(entry.name.as_bytes())
            .map_err(|_| LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::Cleanup))?;
        let named = unsafe { libc::openat(entry.parent.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
        if named < 0 {
            return Err(LocalFileError::from_io(LocalFileOperation::Cleanup, Some(path.to_path_buf()), None, io::Error::last_os_error()));
        }
        // SAFETY: openat returned a fresh descriptor owned here.
        let named_file = unsafe { File::from_raw_fd(named) };
        let named_identity = crate::observation::query::query_observation(&named_file, crate::observation::LocalObservationRequirement::Required)?;
        if named_identity.identity() != Some(expected) {
            return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup));
        }
        let flags = if entry.directory {
            libc::AT_REMOVEDIR
        } else {
            0
        };
        if unsafe { libc::unlinkat(entry.parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
            return Err(LocalFileError::from_io(LocalFileOperation::Cleanup, Some(path.to_path_buf()), None, io::Error::last_os_error()));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO;
        use windows_sys::Win32::Storage::FileSystem::FileDispositionInfo;
        use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        let removed = unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo, (&raw const disposition).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) };
        if removed == 0 {
            return Err(LocalFileError::from_io(LocalFileOperation::Cleanup, Some(path.to_path_buf()), None, io::Error::last_os_error()));
        }
    }
    #[cfg(not(any(unix, windows)))]
    { let _ = (file, expected, path); return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup)); }
    entry.file.take();
    Ok(())
}

impl Drop for LocalTempRecovery {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
