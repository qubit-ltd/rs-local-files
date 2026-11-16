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
    /// Unpublished stage retained for inspection or explicit cleanup.
    stage: Option<crate::directory::LocalStagedDirectory>,
    /// Quota held until the stage owner is released.
    reservation: LocalStagedTreeDiskReservation,
}

impl LocalStagedTreeRecovery {
    /// Creates a recovery owner that keeps the stage and quota alive together.
    ///
    /// # Parameters
    /// * `stage` — unpublished native stage retained after a failed operation.
    /// * `reservation` — quota held while recovery remains possible.
    pub(super) fn new(
        /// Stage retained for inspection and cleanup after publication failure.
        stage: crate::directory::LocalStagedDirectory,
        /// Quota retained alongside the stage.
        reservation: LocalStagedTreeDiskReservation,
    ) -> Self {
        Self {
            stage: Some(stage),
            reservation,
        }
    }

    /// Sets cleanup limits on the retained stage.
    ///
    /// # Parameters
    /// * `limits` — bounds for the later cleanup attempt.
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
    /// Consumes the recovery owner and attempts explicit cleanup.
    ///
    /// # Errors
    /// Returns the native cleanup error together with its retained reservation.
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
    /// Native staged-directory type exposed by this recovery owner.
    type Target = crate::directory::LocalStagedDirectory;

    /// Borrows the native stage for its full recovery-owner lifetime.
    ///
    /// # Returns
    /// The retained native stage.
    fn deref(&self) -> &Self::Target {
        self.stage
            .as_ref()
            .expect("staged recovery owner is present")
    }
}

impl DerefMut for LocalStagedTreeRecovery {
    /// Mutably borrows the native stage to configure recovery behavior.
    ///
    /// # Returns
    /// Mutable access to the retained native stage.
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.stage
            .as_mut()
            .expect("staged recovery owner is present")
    }
}

impl std::fmt::Debug for LocalStagedTreeRecovery {
    /// Formats the stage owner and its retained quota for diagnostics.
    ///
    /// # Parameters
    /// * `formatter` — destination for the diagnostic representation.
    ///
    /// # Returns
    /// Formatting success or the formatter error.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStagedTreeRecovery")
            .field("stage", &self.stage)
            .field("reservation", &self.reservation)
            .finish()
    }
}
