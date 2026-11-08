// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Strict metadata preservation for the Unix candidate.

use std::fs::File;
use std::io;

use super::super::ConditionalEntry;
use crate::local::internal::preserve_atomic_metadata;

/// Copies strict Unix metadata from the observed entry to the publication
/// candidate.
///
/// The observed entry is opened before its metadata is copied, so the source
/// refers to the retained entry rather than a path that could be replaced.
///
/// # Parameters
///
/// * `observed` - Retained entry whose metadata is copied.
/// * `candidate` - Open publication candidate receiving the metadata.
///
/// # Errors
///
/// Returns an I/O error if opening the observed entry or preserving its
/// metadata fails.
pub(super) fn preserve(observed: &ConditionalEntry, candidate: &File) -> io::Result<()> {
    let source = observed.open_reader().map_err(io::Error::other)?;
    preserve_atomic_metadata(source.as_file(), candidate)
}
