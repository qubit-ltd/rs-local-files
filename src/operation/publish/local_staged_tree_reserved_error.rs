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
    error: E,
    reservation: LocalStagedTreeDiskReservation,
}

impl<E> LocalStagedTreeReservedError<E> {
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
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.error
    }
}

impl<E: std::fmt::Debug> std::fmt::Debug for LocalStagedTreeReservedError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStagedTreeReservedError")
            .field("error", &self.error)
            .field("reservation", &self.reservation)
            .finish()
    }
}
