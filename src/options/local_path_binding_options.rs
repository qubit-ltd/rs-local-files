// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::time::Duration;

use super::LocalResolutionLimits;

/// Resource budgets for retaining and verifying a source path binding.
///
/// `None` leaves a budget unbounded or, for `deadline`, configures no time
/// limit. A zero ancestor or path-byte limit is a real zero budget and can
/// make acquisition or verification fail with a resource-limit error. A zero
/// deadline can make time-limited work return a timeout error. These limits do
/// not create an atomic lock: verification reports whether the name
/// still resolves to the same native object, not whether that object's
/// contents have the same version. Use a digest or another content-level
/// check when content identity matters.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use std::time::Duration;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::LocalSourceVerification;
/// use qubit_local_files::options::LocalObservedReadOptions;
/// use qubit_local_files::options::LocalPathBindingOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let scratch = tempfile::tempdir()?;
/// std::fs::write(scratch.path().join("payload"), b"data")?;
/// let filesystem = LocalFileSystem::rooted(scratch.path())?;
/// let limits = LocalPathBindingOptions::new()
///     .with_max_ancestors(8)
///     .with_max_path_bytes(4096)
///     .with_deadline(Duration::from_secs(30));
/// let options = LocalObservedReadOptions::new().with_path_binding(limits);
/// let reader = filesystem.open_observed_reader(Path::new("payload"), &options)?;
/// assert_eq!(reader.verify_path_binding()?, LocalSourceVerification::Unchanged);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "path binding options have no effect unless they are used"]
pub struct LocalPathBindingOptions {
    /// Maximum retained ancestor handles; `None` is unbounded and `Some(0)`
    /// permits none.
    max_ancestors: Option<usize>,
    /// Maximum cumulative native path bytes; `None` is unbounded and `Some(0)`
    /// permits no path bytes.
    max_path_bytes: Option<usize>,
    /// Cooperative duration limit; `None` means no configured deadline and
    /// `Some(Duration::ZERO)` is an immediate deadline.
    deadline: Option<Duration>,
    /// Additional resolution constraints; `None` adds no constraints and
    /// configured limits combine restrictively with enclosing limits.
    resolution_limits: Option<LocalResolutionLimits>,
}

impl LocalPathBindingOptions {
    /// Creates path-binding options without additional limits.
    pub const fn new() -> Self {
        Self {
            max_ancestors: None,
            max_path_bytes: None,
            deadline: None,
            resolution_limits: None,
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
    /// Sets limits for resolving and rechecking the bound path.
    ///
    /// These limits are combined with any enclosing filesystem or handle
    /// limits, so they can only make resolution more restrictive.
    pub const fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.resolution_limits = Some(limits);
        self
    }

    /// Returns the optional ancestor-handle limit without performing I/O;
    /// `None` is unbounded while `Some(0)` permits no retained ancestors.
    pub(crate) const fn max_ancestors(&self) -> Option<usize> {
        self.max_ancestors
    }

    /// Returns the optional path-byte limit without performing I/O;
    /// `None` is unbounded while `Some(0)` permits no path bytes.
    pub(crate) const fn max_path_bytes(&self) -> Option<usize> {
        self.max_path_bytes
    }

    /// Returns the optional deadline without performing I/O; `None` means no
    /// configured deadline and zero is an immediate deadline.
    pub(crate) const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }
    /// Returns additional resolution constraints without performing I/O;
    /// `None` adds no constraints to enclosing limits.
    pub(crate) const fn resolution_limits(&self) -> Option<LocalResolutionLimits> {
        self.resolution_limits
    }
}
