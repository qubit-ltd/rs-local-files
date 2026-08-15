//! Resource limits for directory traversal.

/// Immutable limits applied to one filesystem instance's walks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalWalkLimits {
    max_entries: Option<u64>,
    max_open_handles: Option<u64>,
}

impl LocalWalkLimits {
    /// Creates an unrestricted limit set.
    pub const fn new() -> Self {
        Self {
            max_entries: None,
            max_open_handles: None,
        }
    }

    /// Sets the maximum number of entries a walk may yield.
    pub const fn with_max_entries(mut self, value: u64) -> Self {
        self.max_entries = Some(value);
        self
    }

    /// Sets the maximum number of simultaneously open directory handles.
    pub const fn with_max_open_handles(mut self, value: u64) -> Self {
        self.max_open_handles = Some(value);
        self
    }

    /// Returns the configured entry limit.
    pub const fn max_entries(self) -> Option<u64> {
        self.max_entries
    }

    /// Returns the configured open-handle limit.
    pub const fn max_open_handles(self) -> Option<u64> {
        self.max_open_handles
    }
}

impl Default for LocalWalkLimits {
    fn default() -> Self {
        Self::new()
    }
}
