// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Failure context for descriptor-relative directory enumeration.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::io;

/// An I/O failure classified by whether its directory cursor advanced to a
/// child.
pub(crate) struct RootedDirectoryReadError {
    error: io::Error,
    entry_name: Option<OsString>,
}

impl RootedDirectoryReadError {
    /// Creates an error from directory-stream reading or record parsing.
    pub(crate) fn stream(error: io::Error) -> Self {
        Self {
            error,
            entry_name: None,
        }
    }

    /// Creates an error while inspecting a child after advancing the directory
    /// cursor.
    pub(crate) fn entry(name: OsString, error: io::Error) -> Self {
        Self {
            error,
            entry_name: Some(name),
        }
    }

    /// Returns the child name when its inspection failed after cursor
    /// advancement.
    pub(crate) fn entry_name(&self) -> Option<&OsStr> {
        self.entry_name.as_deref()
    }

    /// Returns the original I/O error without changing its kind or source.
    pub(crate) fn into_io_error(self) -> io::Error {
        self.error
    }
}
