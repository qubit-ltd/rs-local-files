// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Identity-bound cleanup ownership for a newly created Unix private entry.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

use libc::unlinkat;

use super::namespace::Namespace;
use super::namespace::identity_at;
use super::private_entry_setup_error::PrivateEntrySetupError;

/// Owns best-effort cleanup of a newly created private namespace entry.
///
/// The guard records the file identity before cleanup and removes the entry
/// only while its current identity still matches. Dropping an armed guard
/// attempts the same verified cleanup and deliberately ignores cleanup errors.
#[derive(Debug)]
pub(super) struct PrivateEntryGuard<'a> {
    /// Namespace whose retained parent descriptor anchors cleanup.
    namespace: &'a Namespace,
    /// NUL-terminated, single-component name of the created entry.
    name: CString,
    /// Device and inode captured from the created file, when available.
    identity: Option<(u64, u64)>,
    /// Whether dropping this guard should attempt verified cleanup.
    armed: bool,
}

impl<'a> PrivateEntryGuard<'a> {
    /// Creates an armed guard and captures the created file's identity.
    ///
    /// On metadata failure, returns the error together with the still-armed
    /// guard so the caller can attempt cleanup before propagating the error.
    ///
    /// # Parameters
    ///
    /// - `namespace`: retained directory namespace used to anchor cleanup.
    /// - `name`: NUL-terminated component naming the created entry.
    /// - `file`: handle for the entry whose identity is captured.
    ///
    /// # Returns
    ///
    /// Returns the armed guard, or the metadata error paired with that guard.
    pub(super) fn new(namespace: &'a Namespace, name: CString, file: &File) -> Result<Self, (io::Error, Self)> {
        let mut guard = Self {
            namespace,
            name,
            identity: None,
            armed: true,
        };
        #[cfg(test)]
        let metadata = super::test_support::fail_if_requested(super::test_support::FailurePoint::GuardIdentityMetadata)
            .and_then(|()| file.metadata());
        #[cfg(not(test))]
        let metadata = file.metadata();
        match metadata {
            Ok(metadata) => {
                guard.identity = Some((metadata.dev(), metadata.ino()));
                Ok(guard)
            }
            Err(error) => Err((error, guard)),
        }
    }

    /// Relinquishes responsibility for deleting the private entry.
    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    /// Attempts cleanup and combines a cleanup failure with the setup error.
    ///
    /// The returned error retains the primary error kind. If cleanup fails,
    /// the guard remains armed so `Drop` can make one further best-effort
    /// attempt.
    ///
    /// # Parameters
    ///
    /// - `primary`: the error that prevented setup from completing.
    ///
    /// # Returns
    ///
    /// Returns the primary error, or an error describing both setup and
    /// cleanup failures.
    #[must_use]
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
                        path: self.path(),
                    },
                )
            }
        }
    }

    /// Builds the diagnostic path without using it to select the entry.
    ///
    /// # Returns
    ///
    /// Returns the namespace's parent path joined with the entry name for
    /// error reporting.
    fn path(&self) -> PathBuf {
        self.namespace
            .namespace_path
            .parent()
            .unwrap_or(Path::new(""))
            .join(std::ffi::OsStr::from_bytes(self.name.as_bytes()))
    }

    /// Removes the entry only after confirming that its identity is unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if the identity is unavailable or changed, identity
    /// lookup fails, or the operating system cannot remove the entry.
    fn cleanup(&self) -> io::Result<()> {
        let Some(expected) = self.identity else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "cannot verify newly created private entry identity at {}",
                    self.path().display()
                ),
            ));
        };
        if identity_at(&self.namespace.parent, &self.name)? != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "newly created private entry identity changed at {}",
                    self.path().display()
                ),
            ));
        }
        // SAFETY: the parent descriptor is retained and the single-component
        // name is NUL-terminated. Identity was checked immediately above.
        let result = unsafe { unlinkat(self.namespace.parent.as_raw_fd(), self.name.as_ptr(), 0) };
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for PrivateEntryGuard<'_> {
    /// Makes one ignored-error cleanup attempt while the guard remains armed.
    fn drop(&mut self) {
        if self.armed {
            let _ = self.cleanup();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;

    use super::PrivateEntryGuard;
    use crate::local::internal::conditional::unix::namespace::Namespace;

    /// Builds a namespace rooted at a temporary directory for these tests.
    fn namespace(directory: &std::path::Path) -> Namespace {
        Namespace {
            parent: File::open(directory).expect("parent directory should open"),
            target_name: CString::new("target").expect("name should be valid"),
            sync_dirs: Vec::new(),
            namespace_path: directory.join("target"),
            diagnostic_path: Some(directory.join("target")),
        }
    }

    #[test]
    fn test_explicit_setup_failure_cleans_the_verified_private_entry() {
        let directory = tempfile::tempdir().expect("fixture should be created");
        let namespace = namespace(directory.path());
        let (name, mut file) = namespace
            .create_private("candidate")
            .expect("candidate should be created");
        file.write_all(b"candidate").expect("candidate should be writable");
        let mut guard = PrivateEntryGuard::new(&namespace, name.clone(), &file).expect("identity should be captured");

        let error = guard.finish_with(std::io::Error::other("injected entry registration failure"));

        assert!(error.to_string().contains("injected entry registration failure"));
        assert!(
            !directory
                .path()
                .join(std::ffi::OsStr::from_bytes(name.as_bytes()))
                .exists()
        );
    }

    #[test]
    fn test_cleanup_refuses_to_remove_a_replacement_with_unknown_identity() {
        let directory = tempfile::tempdir().expect("fixture should be created");
        let namespace = namespace(directory.path());
        let (name, file) = namespace
            .create_private("candidate")
            .expect("candidate should be created");
        let mut guard = PrivateEntryGuard::new(&namespace, name.clone(), &file).expect("identity should be captured");
        let path = directory.path().join(std::ffi::OsStr::from_bytes(name.as_bytes()));
        std::fs::remove_file(&path).expect("original candidate should be removed by fixture");
        std::fs::write(&path, b"competitor").expect("replacement should be created");

        let error = guard.finish_with(std::io::Error::other("injected metadata failure"));

        assert!(error.to_string().contains("identity changed"));
        assert_eq!(std::fs::read(path).expect("competitor should remain"), b"competitor");
    }
}
