// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Error-source behavior is covered through the public error integration tests.
use std::error::Error;
use std::fmt;
use std::io;

use super::LocalFileError;
use super::LocalPathCodecError;
use super::LocalResourceLimitError;

/// Typed source retained by a local filesystem operation failure.
///
/// Debug output for [`Self::InvalidEncoding`] reports byte-length and decoding
/// position metadata without printing the input body. The typed source still
/// retains the original bytes for callers that explicitly need to inspect them.
///
/// # Examples
///
/// ```
/// use std::io;
///
/// use qubit_local_files::error::LocalFileErrorSource;
///
/// let source = LocalFileErrorSource::Io(io::Error::from(io::ErrorKind::NotFound));
/// assert!(matches!(source, LocalFileErrorSource::Io(_)));
/// ```
#[must_use]
#[non_exhaustive]
pub enum LocalFileErrorSource {
    /// File contents failed UTF-8 decoding.
    InvalidEncoding(
        /// Retained UTF-8 decoding failure.
        std::string::FromUtf8Error,
    ),
    /// Operating-system I/O failure.
    Io(
        /// Retained operating-system error.
        io::Error,
    ),
    /// Canonical native path conversion failure.
    PathCodec(
        /// Retained canonical path conversion error.
        LocalPathCodecError,
    ),
    /// Local resource budget could not satisfy an acquisition request.
    ResourceLimit(
        /// Retained structured resource-budget error.
        LocalResourceLimitError,
    ),
    /// Structured failure retaining an explicit recovery resource.
    Recovery(
        /// The complete error and its owned recovery resource.
        Box<LocalFileError>,
    ),
}

impl fmt::Debug for LocalFileErrorSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEncoding(error) => formatter
                .debug_struct("InvalidEncoding")
                .field("byte_len", &error.as_bytes().len())
                .field("utf8_error", &error.utf8_error())
                .finish(),
            Self::Io(error) => formatter.debug_tuple("Io").field(error).finish(),
            Self::PathCodec(error) => formatter.debug_tuple("PathCodec").field(error).finish(),
            Self::ResourceLimit(error) => formatter.debug_tuple("ResourceLimit").field(error).finish(),
            Self::Recovery(error) => formatter.debug_tuple("Recovery").field(error).finish(),
        }
    }
}

impl fmt::Display for LocalFileErrorSource {
    /// Formats the retained typed failure source.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEncoding(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::PathCodec(error) => error.fmt(formatter),
            Self::ResourceLimit(error) => error.fmt(formatter),
            Self::Recovery(error) => error.fmt(formatter),
        }
    }
}

impl Error for LocalFileErrorSource {
    /// Returns the concrete I/O, codec, or resource-limit error retained here.
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEncoding(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::PathCodec(error) => Some(error),
            Self::ResourceLimit(error) => Some(error),
            Self::Recovery(error) => Some(error.as_ref()),
        }
    }
}
