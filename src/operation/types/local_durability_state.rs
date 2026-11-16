// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Durability fact associated with a published local change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalDurabilityState {
    /// Durability has not been established.
    #[default]
    Unknown,
    /// The configured durability requirement was met.
    Durable,
    /// Publication succeeded, but the durability requirement failed.
    Failed,
}
