// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared bounded scratch-root ownership for filesystem fuzz targets.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

const MAX_ROOT_ATTEMPTS: u64 = 16;
static NEXT_ROOT_ID: AtomicU64 = AtomicU64::new(0);

/// A uniquely created scratch directory removed on scope exit.
pub(crate) struct FuzzRoot {
    path: PathBuf,
}

impl FuzzRoot {
    /// Atomically creates one process-unique scratch directory.
    ///
    /// Returns `None` after a bounded sequence of collisions or ambient I/O
    /// failures so that environmental setup does not become a fuzz finding.
    pub(crate) fn create(label: &str) -> Option<Self> {
        let first_id = NEXT_ROOT_ID.fetch_add(MAX_ROOT_ATTEMPTS, Ordering::Relaxed);
        for offset in 0..MAX_ROOT_ATTEMPTS {
            let path = std::env::temp_dir().join(format!(
                "qubit-local-files-{label}-{}-{}",
                std::process::id(),
                first_id + offset,
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Some(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// Returns the exclusively owned scratch-root path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FuzzRoot {
    /// Performs best-effort cleanup without converting host cleanup failures
    /// into library crash findings.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
