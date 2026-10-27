use std::time::Duration;

/// Resource budgets for retaining and verifying a source path binding.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "path binding options have no effect unless they are used"]
pub struct LocalPathBindingOptions {
    max_ancestors: Option<usize>,
    max_path_bytes: Option<usize>,
    deadline: Option<Duration>,
}

impl LocalPathBindingOptions {
    /// Creates path-binding options without additional limits.
    pub const fn new() -> Self {
        Self {
            max_ancestors: None,
            max_path_bytes: None,
            deadline: None,
        }
    }

    /// Limits retained ancestor directory handles.
    pub const fn with_max_ancestors(mut self, maximum: usize) -> Self {
        self.max_ancestors = Some(maximum);
        self
    }

    /// Limits cumulative native path bytes retained for binding.
    pub const fn with_max_path_bytes(mut self, maximum: usize) -> Self {
        self.max_path_bytes = Some(maximum);
        self
    }

    /// Limits total time spent acquiring and verifying the binding.
    pub const fn with_deadline(mut self, maximum: Duration) -> Self {
        self.deadline = Some(maximum);
        self
    }

    pub(crate) const fn max_ancestors(&self) -> Option<usize> {
        self.max_ancestors
    }

    pub(crate) const fn max_path_bytes(&self) -> Option<usize> {
        self.max_path_bytes
    }

    pub(crate) const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }
}
