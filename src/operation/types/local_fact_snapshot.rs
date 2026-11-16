// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Bounded summary of local filesystem effects, independent of operation
/// outcome.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalFactSnapshot {
    /// Number of known mutation events.
    pub modified_count: u64,
    /// Total number of known modified bytes.
    pub modified_bytes: u64,
    /// A known modified byte range, when representable as one interval.
    pub modified_range: Option<LocalModifiedRange>,
    /// Whether the full mutation range is unknown or non-contiguous.
    pub range_unknown: bool,
    /// Publication status retained even when a generic outcome omits the value.
    pub publication: LocalPublicationState,
    /// Durability status for the published change.
    pub durability: LocalDurabilityState,
    /// Failure to record the effect in the operation manager.
    pub effect_record_error: Option<qubit_operation::EffectRecordError>,
    /// Recovery responsibility associated with this operation, if any.
    pub recovery_id: Option<qubit_operation::RecoveryId>,
}
