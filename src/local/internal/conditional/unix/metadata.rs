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

pub(super) fn preserve(observed: &ConditionalEntry, candidate: &File) -> io::Result<()> {
    let source = observed.open_reader().map_err(io::Error::other)?;
    preserve_atomic_metadata(source.as_file(), candidate)
}
