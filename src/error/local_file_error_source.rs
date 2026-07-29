// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// qubit-style: allow all -- error-source behavior is covered by error
// integration tests.
use std::{error::Error, fmt, io};

use super::LocalPathCodecError;

/// Typed source retained by a local filesystem operation failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum LocalFileErrorSource {
    /// Operating-system I/O failure.
    Io(io::Error),
    /// Canonical native path conversion failure.
    PathCodec(LocalPathCodecError),
}

impl fmt::Display for LocalFileErrorSource {
    /// Formats the retained typed failure source.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::PathCodec(error) => error.fmt(formatter),
        }
    }
}

impl Error for LocalFileErrorSource {
    /// Returns the concrete I/O or codec error retained by this source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::PathCodec(error) => Some(error),
        }
    }
}
