// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ops::Deref;
use std::ops::DerefMut;

use super::LocalStagedTreeDiskReservation;
use super::LocalStagedTreeReservedError;

/// A failed staged write that still owns the unpublished stage and disk quota.
pub struct LocalStagedTreeRecovery {
    stage: Option<crate::directory::LocalStagedDirectory>,
    reservation: LocalStagedTreeDiskReservation,
}

impl LocalStagedTreeRecovery {
    pub(super) fn new(
        stage: crate::directory::LocalStagedDirectory,
        reservation: LocalStagedTreeDiskReservation,
    ) -> Self {
        Self {
            stage: Some(stage),
            reservation,
        }
    }

    /// Sets cleanup limits on the retained stage.
    pub fn set_cleanup_limits(&mut self, limits: crate::options::LocalTempCleanupLimits) {
        self.stage
            .as_mut()
            .expect("staged recovery owner is present")
            .set_cleanup_limits(limits);
    }

    /// Returns the stage name under its captured parent authority.
    pub fn relative_name(&self) -> &std::ffi::OsStr {
        self.stage
            .as_ref()
            .expect("staged recovery owner is present")
            .relative_name()
    }

    /// Explicitly cleans the stage. A failure keeps its native recovery error
    /// and the disk reservation in one value.
    pub fn cleanup(
        mut self,
    ) -> Result<(), LocalStagedTreeReservedError<crate::directory::LocalTreeCleanupError>> {
        let stage = self.stage.take().expect("staged recovery owner is present");
        stage
            .cleanup()
            .map_err(|error| LocalStagedTreeReservedError::new(error, self.reservation))
    }
}

impl Deref for LocalStagedTreeRecovery {
    type Target = crate::directory::LocalStagedDirectory;

    fn deref(&self) -> &Self::Target {
        self.stage
            .as_ref()
            .expect("staged recovery owner is present")
    }
}

impl DerefMut for LocalStagedTreeRecovery {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.stage
            .as_mut()
            .expect("staged recovery owner is present")
    }
}

impl std::fmt::Debug for LocalStagedTreeRecovery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStagedTreeRecovery")
            .field("stage", &self.stage)
            .field("reservation", &self.reservation)
            .finish()
    }
}
