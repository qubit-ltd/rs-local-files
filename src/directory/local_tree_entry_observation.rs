// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;

use crate::observation::LocalFileObservation;

/// Observation of one entry yielded from a staged tree.
#[derive(Clone, Debug)]
pub struct LocalTreeEntryObservation {
    relative_path: PathBuf,
    observation: LocalFileObservation,
}

impl LocalTreeEntryObservation {
    pub(crate) const fn new(relative_path: PathBuf, observation: LocalFileObservation) -> Self {
        Self {
            relative_path,
            observation,
        }
    }

    /// Returns this entry's path relative to the staged directory.
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns the native observation captured for this entry.
    #[must_use]
    pub const fn observation(&self) -> &LocalFileObservation {
        &self.observation
    }
}
