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

/// Retains the created file handle and identity while private-entry setup runs.
///
/// Dropping an armed guard attempts best-effort cleanup only when the namespace
/// entry still refers to the file captured from the retained handle.
#[derive(Debug)]
pub(super) struct PrivateEntryGuard<'a> {
    /// Namespace used to verify and remove the created entry.
    namespace: &'a Namespace,
    /// Open handle whose identity authorizes cleanup.
    file: &'a mut File,
    /// Diagnostic path included if setup and cleanup both fail.
    path: PathBuf,
    /// Identity captured from the created file handle before namespace cleanup.
    identity: Option<FileIdentity>,
    /// Whether Drop still owns best-effort identity-checked cleanup.
    armed: bool,
}

impl<'a> PrivateEntryGuard<'a> {
    /// Creates an armed guard and records the identity of the open file.
    ///
    /// # Parameters
    ///
    /// * `namespace` — namespace in which the private entry was created.
    /// * `name` — entry name used to build its diagnostic path.
    /// * `file` — retained open handle whose identity is captured.
    ///
    /// # Returns
    ///
    /// Returns the guard when identity capture succeeds. If capture fails,
    /// returns the error together with the still-armed guard so cleanup remains
    /// possible during error handling.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file identity cannot be read.
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
    #[inline]
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    /// Combines a setup error with cleanup failure and keeps Drop armed when
    /// cleanup fails.
    ///
    /// # Parameters
    ///
    /// * `primary` — error that caused setup to stop.
    ///
    /// # Returns
    ///
    /// Returns `primary` if cleanup succeeds; otherwise returns an error that
    /// retains both failures and the diagnostic entry path.
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

    /// Borrows the retained handle for identity checks and entry verification.
    ///
    /// # Returns
    ///
    /// Returns the mutable borrow tied to this guard's lifetime.
    #[inline]
    pub(super) fn file_mut(&mut self) -> &mut File {
        self.file
    }

    /// Removes the entry only if it still matches the identity captured from
    /// the retained file handle.
    ///
    /// # Returns
    ///
    /// Returns success after identity-checked removal.
    ///
    /// # Errors
    ///
    /// Returns an error when identity is unavailable or the namespace cannot
    /// verify and remove the matching entry.
    fn cleanup(&self) -> io::Result<()> {
        let Some(identity) = self.identity else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "private entry identity unavailable",
            ));
        };
        self.namespace.delete_created_private(self.file, Some(identity))
    }
}

impl Drop for PrivateEntryGuard<'_> {
    /// Attempts identity-checked best-effort cleanup while the guard is armed.
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
