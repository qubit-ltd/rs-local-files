// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native file write-open modes.
// qubit-style: allow source-test-pair

/// Selects the native creation and positioning behavior for a writer.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(dead_code)]
pub enum Mode {
    /// Opens an existing file at offset zero without truncating it.
    OpenExistingAtStart,
    /// Creates a new file and fails if the target already exists.
    CreateNew,
    /// Creates a missing file or truncates an existing file.
    #[default]
    CreateOrTruncate,
    /// Appends to an existing file and fails if it is missing.
    AppendExisting,
    /// Appends to an existing file or creates it when missing.
    AppendOrCreate,
}
