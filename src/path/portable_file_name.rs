// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Portable local file names.

use std::fmt::{self, Display, Formatter};
use std::io;

/// A validated UTF-8 filename accepted by mainstream local filesystems.
///
/// This type represents one filename component, not a path. Validation rejects
/// separators, dot components, trailing spaces or dots, control characters,
/// and Windows-reserved device names.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PortableFileName(Box<str>);

impl PortableFileName {
    /// Returns the validated filename text.
    ///
    /// # Returns
    /// The single UTF-8 filename component.
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for PortableFileName {
    type Error = io::Error;

    /// Validates and owns one filename component.
    ///
    /// # Errors
    /// Returns `InvalidInput` when the value is empty, reserved, contains a
    /// separator or control character, ends with a space or dot, or exceeds
    /// the portable filename length.
    #[inline]
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        crate::local::validate_portable_file_name_impl(value)?;
        Ok(Self(value.into()))
    }
}

impl TryFrom<String> for PortableFileName {
    type Error = io::Error;

    /// Validates and owns one allocated filename component.
    ///
    /// # Errors
    /// Returns the same validation errors as [`PortableFileName::try_from`] for
    /// a borrowed string.
    #[inline]
    fn try_from(value: String) -> Result<Self, Self::Error> {
        crate::local::validate_portable_file_name_impl(&value)?;
        Ok(Self(value.into_boxed_str()))
    }
}

impl AsRef<str> for PortableFileName {
    /// Borrows the validated filename text.
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for PortableFileName {
    /// Formats the validated filename without escaping it.
    #[inline]
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
