// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalPathBindingOptions;
use super::LocalReadOptions;
use super::LocalResolutionLimits;
use crate::observation::LocalObservationRequirement;

/// Options for opening a reader with same-handle metadata observations.
///
/// An explicit `LocalReadOptions` replaces the filesystem's reader defaults;
/// omitted read options inherit those defaults. Resolution limits have a
/// different rule: they are tightened with filesystem limits, so they can
/// only add restrictions. `None` adds no cap, and a configured zero is a real
/// cap. Finite resolution limits are supported by Rooted observed reads;
/// Host observed reads with a finite effective limit return `Unsupported`.
///
/// # Errors
///
/// See [`LocalDirectoryHandle::open_observed_reader`](crate::directory::LocalDirectoryHandle::open_observed_reader).
/// The method can return `Unsupported` for a finite Host resolution budget,
/// as well as path, policy, observation, resource-limit, and native I/O errors.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use std::path::Path;
/// use qubit_local_files::options::{LocalObservedReadOptions, LocalResolutionLimits};
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let directory = tempfile::tempdir()?;
/// fs::write(directory.path().join("item"), b"observed")?;
/// let filesystem = LocalFileSystem::rooted(directory.path())?;
/// let options = LocalObservedReadOptions::new().with_resolution_limits(
///     LocalResolutionLimits::new().with_max_symlink_hops(0),
/// );
/// let reader = filesystem.open_observed_reader(Path::new("item"), &options)?;
/// assert_eq!(reader.observation().metadata().len(), 8);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "observed-read options have no effect unless they are used"]
pub struct LocalObservedReadOptions {
    /// Optional complete replacement for the filesystem's reader defaults.
    read_options: Option<LocalReadOptions>,
    /// Whether the native identity and timestamp extension is optional or
    /// required; normalized metadata remains required in either case.
    native_observation: LocalObservationRequirement,
    /// Optional limits for retaining and verifying the source path binding.
    path_binding: Option<LocalPathBindingOptions>,
    /// Optional resolution budget, combined restrictively with filesystem
    /// limits.
    resolution_limits: Option<LocalResolutionLimits>,
}

impl LocalObservedReadOptions {
    /// Creates options with optional native details and no retained path
    /// binding.
    ///
    /// # Returns
    ///
    /// Options that inherit reader defaults and apply no additional path
    /// binding or resolution budget.
    #[inline]
    pub const fn new() -> Self {
        Self {
            read_options: None,
            native_observation: LocalObservationRequirement::Optional,
            path_binding: None,
            resolution_limits: None,
        }
    }

    /// Replaces the filesystem's reader options.
    ///
    /// # Parameters
    ///
    /// * `options` — the complete reader configuration to use instead of the
    ///   filesystem defaults.
    ///
    /// # Returns
    ///
    /// The updated options value.
    #[inline]
    pub const fn with_read_options(mut self, options: LocalReadOptions) -> Self {
        self.read_options = Some(options);
        self
    }

    /// Sets whether the complete native identity and timestamp extension is
    /// required.
    ///
    /// The normalized metadata query remains required with either setting.
    /// `Optional` omits the native extension when its query fails; `Required`
    /// reports a Unix native-query error with its I/O cause, or reports a
    /// missing extension as `RequirementNotMet`. On Windows, a failed native
    /// extension query is classified as `RequirementNotMet` while retaining
    /// its source.
    ///
    /// # Parameters
    ///
    /// * `requirement` — whether missing or failed native identity and
    ///   timestamp details may be omitted or must cause an error.
    ///
    /// # Returns
    ///
    /// The updated options value.
    #[inline]
    pub const fn with_native_observation(mut self, requirement: LocalObservationRequirement) -> Self {
        self.native_observation = requirement;
        self
    }

    /// Enables retaining and verifying the source path binding.
    ///
    /// # Parameters
    ///
    /// * `options` — path-binding retention and verification budgets.
    ///
    /// # Returns
    ///
    /// The updated options value.
    #[inline]
    pub const fn with_path_binding(mut self, options: LocalPathBindingOptions) -> Self {
        self.path_binding = Some(options);
        self
    }

    /// Sets limits for resolving the reader's source path.
    ///
    /// The complete limit set on these options is replaced by `limits`.
    /// Resolution then combines it with any filesystem limits, so the
    /// effective budget can only become more restrictive.
    /// Host observed reads return `Unsupported` when the effective resolution
    /// limits are finite, including limits supplied through path binding. An
    /// unbounded Host request keeps its existing behavior. Use a Rooted
    /// filesystem when the observed read must enforce finite resolution limits.
    ///
    /// # Parameters
    ///
    /// * `limits` — the complete per-resolution budget to combine with
    ///   filesystem limits.
    ///
    /// # Returns
    ///
    /// The updated options value.
    #[inline]
    pub const fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.resolution_limits = Some(limits);
        self
    }

    /// Returns the explicit reader configuration, or `None` to inherit the
    /// filesystem defaults.
    ///
    /// # Returns
    ///
    /// `Some` with the configured reader options, or `None` when this value
    /// inherits the filesystem defaults.
    #[inline]
    pub(crate) const fn read_options(&self) -> Option<&LocalReadOptions> {
        self.read_options.as_ref()
    }

    /// Returns whether native identity and timestamp details are optional or
    /// required; normalized metadata is required independently.
    ///
    /// # Returns
    ///
    /// The native observation requirement selected for this read.
    #[must_use]
    #[inline]
    pub(crate) const fn native_observation(&self) -> LocalObservationRequirement {
        self.native_observation
    }

    /// Returns the path-binding configuration, or `None` when no source path
    /// binding is retained.
    ///
    /// # Returns
    ///
    /// `Some` with the retention and verification budgets, or `None` when no
    /// path binding is configured.
    #[inline]
    pub(crate) const fn path_binding(&self) -> Option<&LocalPathBindingOptions> {
        self.path_binding.as_ref()
    }

    /// Returns the additional resolution budget, or `None` when this value
    /// adds no resolution limits.
    ///
    /// # Returns
    ///
    /// `Some` with the per-resolution budget, or `None` when this value adds
    /// no resolution constraints.
    #[inline]
    pub(crate) const fn resolution_limits(&self) -> Option<LocalResolutionLimits> {
        self.resolution_limits
    }
}
