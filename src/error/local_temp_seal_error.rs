use crate::LocalFileError;
use crate::LocalTempAccessState;
use crate::LocalTempFile;

/// Failure while sealing, retaining the temporary resource for recovery.
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
    pub fn cause(&self) -> &LocalFileError { &self.cause }

    /// Returns the resource owner for recovery or cleanup.
    pub fn into_resource(self) -> LocalTempFile {
        let (_, recovery) = self.cause.into_temp_recovery_parts();
        recovery.and_then(crate::temp::types::LocalTempRecovery::into_file)
            .expect("seal failure retains its temporary file")
    }

    /// Returns the access state established when sealing failed.
    pub const fn access_state(&self) -> LocalTempAccessState { self.access_state }
}
