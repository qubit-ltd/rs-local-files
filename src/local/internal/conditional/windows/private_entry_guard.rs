// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Identity-bound cleanup guard for private conditional entries.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::PathBuf;

use super::namespace::FileIdentity;
use super::namespace::Namespace;
use super::namespace::file_identity;
use super::private_entry_setup_error::PrivateEntrySetupError;

#[derive(Debug)]
pub(super) struct PrivateEntryGuard<'a> {
    namespace: &'a Namespace,
    file: &'a mut File,
    path: PathBuf,
    /// Identity captured from the created file handle before namespace cleanup.
    identity: Option<FileIdentity>,
    /// Whether Drop still owns best-effort identity-checked cleanup.
    armed: bool,
}

impl<'a> PrivateEntryGuard<'a> {
    pub(super) fn new(namespace: &'a Namespace, name: &OsStr, file: &'a mut File) -> Result<Self, (io::Error, Self)> {
        let mut guard = Self {
            namespace,
            file,
            path: namespace.child_path(name),
            identity: None,
            armed: true,
        };
        match file_identity(guard.file_mut()) {
            Ok(identity) => {
                guard.identity = Some(identity);
                Ok(guard)
            }
            Err(error) => Err((error, guard)),
        }
    }

    /// Relinquishes cleanup responsibility after verification or successful
    /// cleanup.
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    /// Combines a setup error with cleanup failure and keeps Drop armed on
    /// failure.
    pub(super) fn finish_with(&mut self, primary: io::Error) -> io::Error {
        let cleanup = self.cleanup();
        if cleanup.is_ok() {
            self.disarm();
        }
        match cleanup {
            Ok(()) => primary,
            Err(cleanup) => {
                let kind = primary.kind();
                io::Error::new(
                    kind,
                    PrivateEntrySetupError {
                        primary,
                        cleanup,
                        path: self.path.clone(),
                    },
                )
            }
        }
    }

    fn cleanup(&self) -> io::Result<()> {
        let Some(identity) = self.identity else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "private entry identity unavailable",
            ));
        };
        self.namespace.delete_created_private(self.file, Some(identity))
    }

    pub(super) fn file_mut(&mut self) -> &mut File {
        self.file
    }
}

impl Drop for PrivateEntryGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = self.cleanup();
    }
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.

//    SPDX-License-Identifier: Apache-2.0

//    Licensed under the Apache License, Version 2.0.
// =============================================================================
