// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Additional per-resolution budgets for Rooted path resolution.
///
/// Each `None` leaves that dimension uncapped by this value; `Some(0)` is a
/// real limit and permits no work in that dimension. When combined with
/// filesystem or parent limits, finite values can only tighten the effective
/// budget.
///
/// These limits are enforced by Rooted resolution. Host observed reads return
/// `Unsupported` when their effective resolution limits are finite; unbounded
/// Host observed reads retain their existing behavior.
/// Each limit measures work during resolution, not the size of the final file
/// contents. Limits are combined by taking the tighter finite value, and
/// `Some(0)` remains a real cap. An operation that exceeds an effective cap
/// returns a resource-limit error; a finite cap on Host observed reads is
/// rejected as unsupported rather than silently ignored.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use std::path::Path;
/// use qubit_local_files::options::{LocalResolutionLimits, LocalRootOpenOptions};
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let directory = tempfile::tempdir()?;
/// fs::write(directory.path().join("item"), b"value")?;
/// let limits = LocalResolutionLimits::new().with_max_symlink_hops(4);
/// let options = LocalRootOpenOptions::new().with_resolution_limits(limits);
/// let filesystem = LocalFileSystem::rooted_with_options(directory.path(), &options)?;
/// let reader = filesystem.open_reader(Path::new("item"))?;
/// assert_eq!(reader.metadata().len(), 5);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct LocalResolutionLimits {
    /// Maximum number of symlink expansions permitted during resolution.
    symlink_hops: Option<usize>,
    /// Maximum native-encoded byte length permitted for each symlink target.
    symlink_target_bytes: Option<usize>,
    /// Maximum number of pending and resolved normal path components.
    actual_components: Option<usize>,
    /// Maximum native-encoded byte length of pending and resolved paths.
    resolved_path_bytes: Option<usize>,
}
impl LocalResolutionLimits {
    /// Creates limits with no caller-added caps.
    ///
    /// # Returns
    ///
    /// A budget with every dimension uncapped.
    #[inline]
    pub const fn new() -> Self {
        Self {
            symlink_hops: None,
            symlink_target_bytes: None,
            actual_components: None,
            resolved_path_bytes: None,
        }
    }
    /// Caps symlink hops; zero forbids following any symlink.
    ///
    /// # Parameters
    ///
    /// * `n` — maximum number of symlink expansions; zero disallows all
    ///   expansions.
    ///
    /// # Returns
    ///
    /// The updated budget.
    #[inline]
    pub const fn with_max_symlink_hops(mut self, n: usize) -> Self {
        self.symlink_hops = Some(n);
        self
    }
    /// Caps native encoded bytes in each symlink target; zero permits no target
    /// bytes.
    ///
    /// # Parameters
    ///
    /// * `n` — maximum native-encoded byte length of an individual symlink
    ///   target; zero permits only empty targets.
    ///
    /// # Returns
    ///
    /// The updated budget.
    #[inline]
    pub const fn with_max_symlink_target_bytes(mut self, n: usize) -> Self {
        self.symlink_target_bytes = Some(n);
        self
    }
    /// Caps pending and resolved path components; zero permits no normal
    /// components.
    ///
    /// # Parameters
    ///
    /// * `n` — maximum number of normal components considered during
    ///   resolution; zero permits none.
    ///
    /// # Returns
    ///
    /// The updated budget.
    #[inline]
    pub const fn with_max_actual_components(mut self, n: usize) -> Self {
        self.actual_components = Some(n);
        self
    }
    /// Caps pending and resolved native path bytes; zero permits no path bytes.
    ///
    /// # Parameters
    ///
    /// * `n` — maximum native-encoded byte length of pending and resolved
    ///   paths; zero permits no path bytes.
    ///
    /// # Returns
    ///
    /// The updated budget.
    #[inline]
    pub const fn with_max_resolved_path_bytes(mut self, n: usize) -> Self {
        self.resolved_path_bytes = Some(n);
        self
    }

    /// Returns the symlink expansion cap set by this budget.
    ///
    /// # Returns
    ///
    /// `Some(limit)` when capped, or `None` when this budget leaves expansion
    /// count uncapped.
    #[must_use]
    #[inline]
    pub(crate) const fn symlink_hops(&self) -> Option<usize> {
        self.symlink_hops
    }
    /// Returns the per-target native byte cap set by this budget.
    ///
    /// # Returns
    ///
    /// `Some(limit)` when capped, or `None` when this budget leaves target
    /// length uncapped.
    #[must_use]
    #[inline]
    pub(crate) const fn symlink_target_bytes(&self) -> Option<usize> {
        self.symlink_target_bytes
    }
    /// Returns the path-component cap set by this budget.
    ///
    /// # Returns
    ///
    /// `Some(limit)` when capped, or `None` when this budget leaves component
    /// count uncapped.
    #[must_use]
    #[inline]
    pub(crate) const fn actual_components(&self) -> Option<usize> {
        self.actual_components
    }
    /// Returns the pending and resolved path byte cap set by this budget.
    ///
    /// # Returns
    ///
    /// `Some(limit)` when capped, or `None` when this budget leaves path length
    /// uncapped.
    #[must_use]
    #[inline]
    pub(crate) const fn resolved_path_bytes(&self) -> Option<usize> {
        self.resolved_path_bytes
    }

    /// Intersects this budget with a child budget, treating `None` as
    /// unlimited.
    ///
    /// # Parameters
    ///
    /// * `child` — additional limits to apply beneath this budget.
    ///
    /// # Returns
    ///
    /// A budget containing the tighter finite cap in each dimension.
    #[inline]
    pub(crate) const fn tightened(self, child: Self) -> Self {
        Self {
            symlink_hops: min_limit(self.symlink_hops, child.symlink_hops),
            symlink_target_bytes: min_limit(self.symlink_target_bytes, child.symlink_target_bytes),
            actual_components: min_limit(self.actual_components, child.actual_components),
            resolved_path_bytes: min_limit(self.resolved_path_bytes, child.resolved_path_bytes),
        }
    }
}

/// Returns the tighter finite cap, treating an absent cap as unlimited.
///
/// # Parameters
///
/// * `parent` — cap inherited from an ancestor, if present.
/// * `child` — additional cap introduced by a child, if present.
///
/// # Returns
///
/// The smaller cap when both are present, the one present cap, or `None` when
/// neither input imposes a cap.
#[inline]
const fn min_limit(parent: Option<usize>, child: Option<usize>) -> Option<usize> {
    match (parent, child) {
        (Some(parent), Some(child)) => Some(if parent < child { parent } else { child }),
        (Some(parent), None) => Some(parent),
        (None, Some(child)) => Some(child),
        (None, None) => None,
    }
}
