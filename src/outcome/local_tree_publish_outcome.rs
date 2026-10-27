// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::directory::LocalTreeRetainedResource;
use crate::error::LocalFileError;
use crate::outcome::LocalTreeAccessState;
use crate::outcome::LocalTreePublication;

/// Reports publication and synchronization facts for a staged tree.
#[derive(Debug)]
pub struct LocalTreePublishOutcome {
    publication: LocalTreePublication,
    durable: bool,
    file_synced: bool,
    access_state: LocalTreeAccessState,
    cleanup_errors: Vec<LocalFileError>,
    retained: Vec<LocalTreeRetainedResource>,
}
impl LocalTreePublishOutcome {
    pub(crate) fn new(
        publication: LocalTreePublication,
        durable: bool,
        file_synced: bool,
        access_state: LocalTreeAccessState,
    ) -> Self {
        Self {
            publication,
            durable,
            file_synced,
            access_state,
            cleanup_errors: Vec::new(),
            retained: Vec::new(),
        }
    }
    /// Returns whether publication completed.
    pub const fn publication(&self) -> LocalTreePublication {
        self.publication
    }
    /// Reports whether the requested parent namespace durability was achieved.
    pub const fn durable(&self) -> bool {
        self.durable
    }
    /// Reports whether requested regular file synchronization completed.
    pub const fn file_synced(&self) -> bool {
        self.file_synced
    }
    /// Returns the final access state confirmed before publication.
    pub const fn access_state(&self) -> LocalTreeAccessState {
        self.access_state
    }
    /// Returns secondary cleanup errors observed after publication.
    pub fn cleanup_errors(&self) -> &[LocalFileError] {
        &self.cleanup_errors
    }
    /// Returns resources retained for explicit recovery.
    pub fn retained_resources(&self) -> &[LocalTreeRetainedResource] {
        &self.retained
    }
    /// Transfers all retained recovery resources.
    pub fn into_retained_resources(mut self) -> Vec<LocalTreeRetainedResource> {
        std::mem::take(&mut self.retained)
    }
}
