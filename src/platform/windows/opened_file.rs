// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows opened-file observations.

use std::fs::File;
use std::io;
use std::io::Read;

use super::EntryIdentity;
use crate::LocalFileMetadata;

/// A regular file together with same-handle metadata and identity.
#[derive(Debug)]
#[must_use]
pub(crate) struct OpenedFile {
    /// Owned native handle.
    file: File,
    /// Metadata captured from the handle.
    metadata: LocalFileMetadata,
    /// Identity captured from the same handle.
    identity: EntryIdentity,
}

impl OpenedFile {
    /// Creates an opened-file observation from one verified handle.
    pub(super) const fn new(file: File, metadata: LocalFileMetadata, identity: EntryIdentity) -> Self {
        Self {
            file,
            metadata,
            identity,
        }
    }

    /// Returns the retained file handle.
    #[must_use]
    pub(crate) const fn file(&self) -> &File {
        &self.file
    }

    /// Returns the retained file handle mutably.
    #[must_use]
    pub(crate) const fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    /// Returns metadata captured from the retained handle.
    pub(crate) const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }

    /// Returns identity captured from the retained handle.
    pub(crate) const fn identity(&self) -> &EntryIdentity {
        &self.identity
    }

    /// Consumes this value and returns the owned file handle.
    #[must_use]
    pub(crate) fn into_file(self) -> File {
        self.file
    }
}

impl Read for OpenedFile {
    /// Reads bytes from the retained handle into `buffer`.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}
