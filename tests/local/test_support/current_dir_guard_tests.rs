// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Process current-directory serialization and restoration for tests.

use std::path::{
    Path,
    PathBuf,
};
use std::sync::Mutex;

/// Serializes tests that temporarily change the process current directory.
pub(crate) static CURRENT_DIR_LOCK: Mutex<()> = Mutex::new(());

/// Restores the process current directory when dropped.
pub(crate) struct CurrentDirGuard {
    /// Original current directory restored by [`Drop`].
    original: PathBuf,
}

impl CurrentDirGuard {
    /// Changes the process current directory and returns a restoration guard.
    ///
    /// # Arguments
    ///
    /// * `path` - Directory to make current until the guard is dropped.
    ///
    /// # Returns
    ///
    /// A guard retaining the original current directory.
    ///
    /// # Panics
    ///
    /// Panics when the current directory cannot be read or changed.
    pub(crate) fn change_to(path: &Path) -> Self {
        let original =
            std::env::current_dir().expect("current dir should be readable");
        std::env::set_current_dir(path).expect("current dir should be changed");
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    /// Best-effort restores the original process current directory.
    fn drop(&mut self) {
        drop(std::env::set_current_dir(&self.original));
    }
}
