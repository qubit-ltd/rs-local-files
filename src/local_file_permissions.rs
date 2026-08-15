//! Portable permission observations for native metadata.

/// Permissions exposed by a local metadata observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFilePermissions {
    read_only: bool,
    unix_mode: Option<u32>,
}

impl LocalFilePermissions {
    /// Creates a permission observation.
    pub const fn new(read_only: bool, unix_mode: Option<u32>) -> Self {
        Self {
            read_only,
            unix_mode,
        }
    }

    /// Reports whether the native entry is read-only.
    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// Returns Unix mode bits when the platform exposes them.
    pub const fn unix_mode(self) -> Option<u32> {
        self.unix_mode
    }
}
