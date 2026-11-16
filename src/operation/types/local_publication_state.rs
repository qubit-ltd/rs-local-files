// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Whether an operation published its destination.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalPublicationState {
    /// No publication fact has been recorded.
    #[default]
    NotPublished,
    /// Publication completed.
    Published,
    /// The publication result could not be established.
    Unknown,
}
