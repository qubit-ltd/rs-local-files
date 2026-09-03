// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable native filesystem path limits.

use super::LocalPathLengthUnit;
use super::SizeLimit;

/// Native path limits observed for one filesystem authority.
///
/// The numeric observations must always be interpreted in
/// [`LocalPathLengthUnit`].
///
/// # Examples
///
/// ```
/// use qubit_local_files::capability::{
///     LocalFileSystemLimits, LocalPathLengthUnit, SizeLimit,
/// };
///
/// let limits = LocalFileSystemLimits::new(
///     SizeLimit::Unknown,
///     SizeLimit::Maximum(255),
///     LocalPathLengthUnit::Bytes,
/// );
/// assert_eq!(limits.max_component_length(), SizeLimit::Maximum(255));
/// assert_eq!(limits.length_unit(), LocalPathLengthUnit::Bytes);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileSystemLimits {
    /// Maximum complete-path length observed from the native authority.
    max_path_length: SizeLimit,
    /// Maximum single-component length observed from the native authority.
    max_component_length: SizeLimit,
    /// Native unit shared by both length observations.
    length_unit: LocalPathLengthUnit,
}

impl LocalFileSystemLimits {
    /// Creates limits from independently observed native dimensions.
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn new(
        max_path_length: SizeLimit,
        max_component_length: SizeLimit,
        length_unit: LocalPathLengthUnit,
    ) -> Self {
        Self {
            max_path_length,
            max_component_length,
            length_unit,
        }
    }

    /// Returns the maximum complete native path length in
    /// [`Self::length_unit`].
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn max_path_length(&self) -> SizeLimit {
        self.max_path_length
    }

    /// Returns the maximum native component length in [`Self::length_unit`].
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn max_component_length(&self) -> SizeLimit {
        self.max_component_length
    }

    /// Returns the unit shared by both observed length dimensions.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn length_unit(&self) -> LocalPathLengthUnit {
        self.length_unit
    }
}
