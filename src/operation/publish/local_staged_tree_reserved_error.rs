// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ops::Deref;

use super::LocalStagedTreeDiskReservation;

/// Keeps a native staged-tree error and its disk reservation together.
pub struct LocalStagedTreeReservedError<E> {
    /// Original native failure, including its recovery facts.
    error: E,
    /// Disk quota retained while the error owns native recovery state.
    reservation: LocalStagedTreeDiskReservation,
}

impl<E> LocalStagedTreeReservedError<E> {
    /// Couples a native error with the quota that must remain reserved.
    ///
    /// # Parameters
    /// * `error` — native failure that owns recovery state.
    /// * `reservation` — quota retained for that state.
    pub(super) fn new(error: E, reservation: LocalStagedTreeDiskReservation) -> Self {
        Self { error, reservation }
    }

    /// Borrows the original native error and its recovery facts.
    pub fn error(&self) -> &E {
        &self.error
    }

    /// Transfers both the native error owner and its disk reservation.
    pub fn into_parts(self) -> (E, LocalStagedTreeDiskReservation) {
        (self.error, self.reservation)
    }
}

impl<E> Deref for LocalStagedTreeReservedError<E> {
    /// Native error type exposed through dereferencing.
    type Target = E;

    /// Borrows the wrapped native error.
    fn deref(&self) -> &Self::Target {
        &self.error
    }
}

impl<E: std::fmt::Debug> std::fmt::Debug for LocalStagedTreeReservedError<E> {
    /// Formats the native error and retained quota.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStagedTreeReservedError")
            .field("error", &self.error)
            .field("reservation", &self.reservation)
            .finish()
    }
}
