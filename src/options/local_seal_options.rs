use crate::policy::LocalFileSyncRequirement;

/// Options controlling the synchronization requested when sealing a temp file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "seal options have no effect unless they are used"]
pub struct LocalSealOptions {
    file_sync: LocalFileSyncRequirement,
}

impl LocalSealOptions {
    /// Creates seal options that require file synchronization.
    #[inline]
    pub const fn new() -> Self {
        Self {
            file_sync: LocalFileSyncRequirement::Required,
        }
    }

    /// Returns the requested file synchronization.
    #[inline]
    pub const fn file_sync(&self) -> LocalFileSyncRequirement {
        self.file_sync
    }

    /// Sets the requested file synchronization.
    #[inline]
    pub const fn with_file_sync(mut self, requirement: LocalFileSyncRequirement) -> Self {
        self.file_sync = requirement;
        self
    }
}

impl Default for LocalSealOptions {
    fn default() -> Self {
        Self::new()
    }
}
