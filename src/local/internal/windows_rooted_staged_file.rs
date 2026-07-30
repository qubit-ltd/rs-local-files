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

use std::{
    fs::File,
    io,
    path::{
        Path,
        PathBuf,
    },
};

use crate::LocalRelativePath;

use super::remove_rooted_entry;

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
    /// Panics if the staging handle was closed before the staging name was
    /// disarmed.
    #[must_use]
    #[inline(always)]
    pub(in crate::local) fn file(&self) -> &File {
        self.file
            .as_ref()
            .expect("rooted staging file must remain open while armed")
    }

    /// Returns the open staging file mutably.
    ///
    /// # Panics
    ///
    /// Panics if the staging handle was closed before the staging name was
    /// disarmed.
    #[must_use]
    #[inline(always)]
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
                permissions.set_readonly(false);
                file.set_permissions(permissions)?;
            }
        }
        self.file.take();
        if self.armed {
            remove_rooted_entry(&self.root, Path::new(""), &self.path, false)?;
            self.armed = false;
        }
        Ok(())
    }

    /// Marks the staging name as installed.
    #[inline(always)]
    pub(in crate::local) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for WindowsRootedStagedFile {
    /// Performs best-effort cleanup for an armed staging entry.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
