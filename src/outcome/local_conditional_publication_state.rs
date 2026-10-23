// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Observed state of the destination when a conditional commit terminates.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocalConditionalPublicationState {
    /// No publication operation changed the destination.
    NotPublished,
    /// The candidate was published at the destination.
    Published,
    /// A publication occurred and the previous destination was restored.
    Restored,
    /// The backend cannot determine the final destination state.
    Indeterminate,
}
