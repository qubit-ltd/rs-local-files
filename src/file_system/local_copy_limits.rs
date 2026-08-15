//! Resource limits for copy operations.

/// Immutable limits applied to one filesystem instance's copies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalCopyLimits {
    max_entries: Option<u64>,
    max_open_handles: Option<u64>,
    max_bytes: Option<u64>,
}

impl LocalCopyLimits {
    /// Creates an unrestricted limit set.
    pub const fn new() -> Self {
        Self {
            max_entries: None,
            max_open_handles: None,
            max_bytes: None,
        }
    }

    /// Sets the maximum number of copied entries.
    pub const fn with_max_entries(mut self, value: u64) -> Self {
        self.max_entries = Some(value);
        self
    }

    /// Sets the maximum number of simultaneously open handles.
    pub const fn with_max_open_handles(mut self, value: u64) -> Self {
        self.max_open_handles = Some(value);
        self
    }

    /// Sets the maximum number of copied bytes.
    pub const fn with_max_bytes(mut self, value: u64) -> Self {
        self.max_bytes = Some(value);
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

    /// Returns the configured byte limit.
    pub const fn max_bytes(self) -> Option<u64> {
        self.max_bytes
    }
}

impl Default for LocalCopyLimits {
    fn default() -> Self {
        Self::new()
    }
}
