// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// qubit-style: allow source-test-pair
// Error-source behavior is covered through the public error integration tests.
use std::error::Error;
use std::fmt;
use std::io;

use super::LocalPathCodecError;
use super::LocalResourceLimitError;

/// Typed source retained by a local filesystem operation failure.
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
#[derive(Debug)]
#[non_exhaustive]
pub enum LocalFileErrorSource {
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
}

impl fmt::Display for LocalFileErrorSource {
    /// Formats the retained typed failure source.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::PathCodec(error) => error.fmt(formatter),
            Self::ResourceLimit(error) => error.fmt(formatter),
        }
    }
}

impl Error for LocalFileErrorSource {
    /// Returns the concrete I/O or codec error retained by this source.
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::PathCodec(error) => Some(error),
            Self::ResourceLimit(error) => Some(error),
        }
    }
}
