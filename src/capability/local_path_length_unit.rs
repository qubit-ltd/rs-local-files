// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Units used by native filesystem path-length observations.

/// Unit in which a native filesystem reports path and component lengths.
///
/// A reported limit is meaningful only together with this unit. In
/// particular, UTF-16 code-unit limits cannot be converted into UTF-8 byte
/// limits without inspecting the concrete path.
///
/// # Examples
///
/// ```
/// use qubit_local_files::capability::LocalPathLengthUnit;
///
/// assert_ne!(
///     LocalPathLengthUnit::Bytes,
///     LocalPathLengthUnit::Utf16CodeUnits,
/// );
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalPathLengthUnit {
    /// Lengths are measured in bytes in the platform-native encoding.
    Bytes,
    /// Lengths are measured in UTF-16 code units.
    Utf16CodeUnits,
}

impl LocalPathLengthUnit {
    /// Returns the native path-observation unit for the current target.
    pub(crate) const fn native() -> Self {
        #[cfg(windows)]
        {
            Self::Utf16CodeUnits
        }
        #[cfg(not(windows))]
        {
            Self::Bytes
        }
    }
}
