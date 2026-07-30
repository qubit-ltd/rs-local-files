// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.

/// Metadata preservation requested during native copy or persistence.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalMetadataPreservePolicy {
    /// Do not copy source metadata.
    #[default]
    None,
    /// Preserve portable native permission metadata.
    Permissions,
}
