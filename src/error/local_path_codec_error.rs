// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::fmt;

/// Failure while converting between canonical path text and a native path
/// component.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalPathCodecError {
    /// An escape did not contain two uppercase hexadecimal digits.
    InvalidEscape {
        /// Byte offset of the percent character that starts the escape.
        offset: usize,
    },
    /// Text decodes successfully but is not the unique canonical spelling.
    NonCanonicalText,
    /// A native path value contains a NUL character or code unit.
    NativeNul,
    /// The current platform cannot expose the required native representation.
    UnsupportedNativeEncoding,
    /// Canonical text cannot be converted to a native path representation.
    UnrepresentableNativeValue,
}

impl fmt::Display for LocalPathCodecError {
    /// Formats the canonical path codec failure for diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEscape { offset } => {
                write!(formatter, "invalid path escape at byte offset {offset}")
            }
            Self::NonCanonicalText => {
                formatter.write_str("non-canonical path text")
            }
            Self::NativeNul => {
                formatter.write_str("native path value contains NUL")
            }
            Self::UnsupportedNativeEncoding => {
                formatter.write_str("native path encoding is unsupported")
            }
            Self::UnrepresentableNativeValue => formatter
                .write_str("path text cannot represent a native path value"),
        }
    }
}

impl Error for LocalPathCodecError {}
