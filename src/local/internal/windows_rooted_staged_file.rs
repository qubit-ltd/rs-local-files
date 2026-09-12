// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows rooted staging-file lifecycle.
// qubit-style: allow source-test-pair
// Covered through the public rooted atomic-writer integration tests.

use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::remove_rooted_entry;
use crate::LocalRelativePath;

/// Owns a Windows rooted staging file and removes its name unless disarmed.
#[must_use = "discarding an armed staging file triggers best-effort cleanup"]
#[derive(Debug)]
pub(in crate::local) struct WindowsRootedStagedFile {
    /// Root capability used for cleanup and installation.
    pub(in crate::local) root: File,
    /// Validated staging path beneath `root`.
    pub(in crate::local) path: LocalRelativePath,
    /// Diagnostic-only absolute staging path.
    pub(in crate::local) diagnostic_path: PathBuf,
    /// Open staging handle.
    pub(in crate::local) file: Option<File>,
    /// Whether the staging name still requires cleanup.
    pub(in crate::local) armed: bool,
}

impl WindowsRootedStagedFile {
    /// Returns the open staging file.
    ///
    /// # Panics
    ///
    /// Panics if the staging handle has already been closed.
    #[must_use]
    #[inline]
    pub(in crate::local) fn file(&self) -> &File {
        self.file
            .as_ref()
            .expect("rooted staging file must remain open while armed")
    }

    /// Returns the open staging file mutably.
    ///
    /// # Panics
    ///
    /// Panics if the staging handle has already been closed.
    #[must_use]
    #[inline]
    pub(in crate::local) fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("rooted staging file must remain open while armed")
    }

    /// Closes and removes the staging entry.
    ///
    /// # Errors
    ///
    /// Returns the native metadata, permission, or removal error encountered
    /// while cleaning up the staging entry.
    pub(in crate::local) fn cleanup(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_ref() {
            let mut permissions = file.metadata()?.permissions();
            if permissions.readonly() {
                // This Windows-only cleanup clears the read-only attribute;
                // the Unix world-writable warning does not apply here.
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                file.set_permissions(permissions)?;
            }
        }
        self.file.take();
        if self.armed {
            remove_rooted_entry(&self.root, Path::new(""), &self.path)?;
            self.armed = false;
        }
        Ok(())
    }

    /// Closes the data handle and marks the staging name as installed.
    /// Closing here prevents later cleanup from changing the published file's
    /// read-only attribute through a retained staging handle.
    #[inline]
    pub(in crate::local) fn disarm(&mut self) {
        drop(self.file.take());
        self.armed = false;
    }
}

impl Drop for WindowsRootedStagedFile {
    /// Performs best-effort cleanup for an armed staging entry.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::WindowsRootedStagedFile;
    use crate::LocalRelativePath;

    /// A published file must retain its read-only attribute after the staging
    /// guard relinquishes cleanup ownership.
    #[test]
    fn test_disarmed_cleanup_preserves_published_read_only_attribute() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let destination = directory.path().join("published");
        let file = fs::File::create(&destination).expect("published fixture should be created");
        let mut permissions = file
            .metadata()
            .expect("fixture metadata should be readable")
            .permissions();
        permissions.set_readonly(true);
        file.set_permissions(permissions)
            .expect("fixture should become read-only");
        let root = crate::local::internal::open_root_directory(directory.path()).expect("root should open");
        let mut staging = WindowsRootedStagedFile {
            root,
            path: LocalRelativePath::new("published").expect("fixture path should be valid"),
            diagnostic_path: destination.clone(),
            file: Some(file),
            armed: true,
        };

        staging.disarm();
        staging.cleanup().expect("disarmed cleanup should succeed");
        drop(staging);
        let mut permissions = fs::metadata(&destination)
            .expect("published entry must remain")
            .permissions();
        let preserved_read_only = permissions.readonly();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&destination, permissions).expect("fixture should be writable for cleanup");

        assert!(
            preserved_read_only,
            "disarmed cleanup must not mutate the published entry"
        );
    }
}
