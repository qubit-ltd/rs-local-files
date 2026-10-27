// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Progress of a directory-tree mutation operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTreeMutationState {
    /// No namespace mutation was attempted.
    NotStarted,
    /// A namespace mutation was attempted.
    Started,
    /// The mutation result could not be determined.
    Indeterminate,
}
