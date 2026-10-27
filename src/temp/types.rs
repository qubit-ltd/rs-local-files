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
    resource: RecoveryResource,
}

#[derive(Debug)]
enum RecoveryResource {
    LiveFile(super::LocalTempFile),
    Native(NativePrivateEntry),
}

impl LocalTempRecovery {
    pub(crate) fn from_file(resource: super::LocalTempFile) -> Self {
        let path = resource.path().to_path_buf();
        let identity = resource.current_identity();
        Self {
            path,
            identity,
            resource: RecoveryResource::LiveFile(resource),
        }
    }

    pub(crate) fn from_native(path: PathBuf, entry: NativePrivateEntry) -> Self {
        Self {
            path,
            identity: entry.identity.clone(),
            resource: RecoveryResource::Native(entry),
        }
    }

    pub(crate) fn into_file(self) -> Option<super::LocalTempFile> {
        match self.resource {
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
        match &mut self.resource {
            RecoveryResource::LiveFile(resource) => resource.cleanup(),
            RecoveryResource::Native(entry) => {
                let Some(expected) = self.identity.as_ref() else {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::RequirementNotMet,
                        crate::LocalFileOperation::Cleanup,
                    ));
                };
                let Some(file) = entry.file.as_ref() else {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::RequirementNotMet,
                        crate::LocalFileOperation::Cleanup,
                    ));
                };
                let observed = crate::observation::query::query_observation(
                    file,
                    crate::observation::LocalObservationRequirement::Required,
                )?;
                if observed.identity() != Some(expected) {
                    return Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::RequirementNotMet,
                        crate::LocalFileOperation::Cleanup,
                    ));
                }
                #[cfg(unix)]
                {
                    use std::os::fd::AsRawFd;
                    use std::os::unix::ffi::OsStrExt;
                    let name = std::ffi::CString::new(entry.name.as_bytes()).map_err(|_| {
                        crate::LocalFileError::new(
                            crate::LocalFileErrorKind::InvalidPath,
                            crate::LocalFileOperation::Cleanup,
                        )
                    })?;
                    if unsafe { libc::unlinkat(entry.parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                        return Err(crate::LocalFileError::from_io(
                            crate::LocalFileOperation::Cleanup,
                            Some(self.path.clone()),
                            None,
                            io::Error::last_os_error(),
                        ));
                    }
                    entry.file.take();
                    Ok(())
                }
                #[cfg(not(unix))]
                {
                    Err(crate::LocalFileError::new(
                        crate::LocalFileErrorKind::RequirementNotMet,
                        crate::LocalFileOperation::Cleanup,
                    ))
                }
            }
        }
    }
}
